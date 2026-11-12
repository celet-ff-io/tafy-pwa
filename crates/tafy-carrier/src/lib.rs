//! The browser leg of the relay conversation (batch 3).
//!
//! `Carrier` + `MailSession` implemented over the gRPC-Web envelope
//! (the protocol's ruled browser face, R32: unary + server-streaming
//! only) via `tonic-web-wasm-client`. The visit routine, the pumps
//! and the seal task stay the library's — this crate only bridges
//! the wire.
//!
//! The Send bridge: the traits promise `Send` futures; the js-bound
//! client's futures are `!Send`. The bridge = `send_wrapper::
//! SendWrapper` around (a) each call's future and (b) the plane
//! streams — sound under the wasm single-threaded executor, exactly
//! the SPI note's SendWrapper-class pattern.
//!
//! Native builds compile an inert stub (the crate never runs off
//! wasm; the workspace's native `cargo check` still passes).

#[cfg(target_arch = "wasm32")]
mod imp {
    use futures::Stream;
    use futures::StreamExt as _;
    use send_wrapper::SendWrapper;
    use tonic::Request;
    use zconned::transport::{Carrier, Error, MailSession, Result};
    use zconnor_mail_types::relay::mailbox_service_client::MailboxServiceClient;
    use zconnor_mail_types::{SESSION_TOKEN_HEADER, mail, relay};

    /// The connection factory: one gRPC-Web client per endpoint
    /// (the relay URL — https, grpc-web envelope).
    pub struct GrpcWebCarrier {
        endpoint: String,
    }

    // ---- the plane-open flake absorb (106's §5 verdict, 2026-11-15) ----
    //
    // The attach OPEN intermittently never answers: the browser↔tunnel
    // pooled h2 connection is idle-reaped between page operations and
    // browsers never auto-retry a non-idempotent POST on a dead
    // connection. The re-test data: attached streams are stable, only
    // the OPEN flakes, and every RETRY opens a fresh pooled connection
    // and connects. So each plane-OPEN RPC races a SHORT timeout and
    // retries PROMPTLY — the hang case resolves inside the visit
    // instead of surfacing to the user (the visit-level gate stays
    // the long backstop: the ceremony legitimately runs tens of
    // seconds through the tunnel).
    const PLANE_OPEN_TIMEOUT_MS: i32 = 10_000;
    const PLANE_OPEN_ATTEMPTS: u32 = 3;

    /// The worker-context timer (the same pattern as the worker's
    /// wasm_sleep): a setTimeout promise as the racing arm.
    async fn sleep_ms(ms: i32) {
        use web_sys::wasm_bindgen::JsCast as _;
        let p = js_sys::Promise::new(&mut |resolve, _| {
            let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
            let _ = scope.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        });
        let _ = SendWrapper::new(wasm_bindgen_futures::JsFuture::from(p)).await;
    }

    /// Race one plane-open future against the short timeout; on a
    /// timeout drop the future (aborting its fetch — the client's
    /// abort guard) and retry on a fresh connection. Exhausting the
    /// attempts surfaces the error (the queued mail stays queued).
    async fn open_plane_rpc<T, Fut, F>(make: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        for attempt in 1..=PLANE_OPEN_ATTEMPTS {
            let fut = make();
            let arm = sleep_ms(PLANE_OPEN_TIMEOUT_MS);
            match futures::future::select(Box::pin(fut), Box::pin(arm)).await {
                futures::future::Either::Left((r, _)) => return r,
                futures::future::Either::Right((_, _fut)) => {
                    trace(&format!(
                        "plane open hit the {}s gate (attempt {attempt}/{PLANE_OPEN_ATTEMPTS}) — retrying on a fresh connection",
                        PLANE_OPEN_TIMEOUT_MS / 1000,
                    ));
                }
            }
        }
        Err(Error::Call {
            message: format!(
                "the plane open never answered within {}s×{PLANE_OPEN_ATTEMPTS} — the pooled connection is dead and the retries exhausted",
                PLANE_OPEN_TIMEOUT_MS / 1000,
            ),
        })
    }

    impl GrpcWebCarrier {
        pub fn new(endpoint: impl Into<String>) -> Self {
            Self {
                endpoint: endpoint.into(),
            }
        }
    }

    impl Carrier for GrpcWebCarrier {
        type Session = GrpcWebSession;

        async fn open(&self) -> Result<Self::Session> {
            trace("carrier open enter");
            let endpoint = self.endpoint.clone();
            // the js-bound construction itself rides the bridge
            let made = SendWrapper::new(async move {
                let channel = tonic_web_wasm_client::Client::new(endpoint);
                MailboxServiceClient::new(channel)
            })
            .await;
            trace("carrier open exit");
            Ok(GrpcWebSession(SendWrapper::new(made), None))
        }
    }

    /// Present the claim-issued credential: every session-bound call
    /// carries it (the 106 mail-session batch — the relay
    /// authenticates the session by the token, never by the
    /// connection, so the tunnel's connection-hopping stays
    /// harmless). A free fn because the token is captured into the
    /// !Send call future ahead of the js state it rides.
    fn present<T>(token: &Option<String>, inner: T) -> Request<T> {
        let mut request = Request::new(inner);
        if let Some(token) = token
            && let Ok(value) = token.clone().try_into()
        {
            request.metadata_mut().insert(SESSION_TOKEN_HEADER, value);
        }
        request
    }

    /// The relay conversation: thin delegation over the generated
    /// `MailboxServiceClient` (the TonicSession is the reference
    /// shape; only the channel type differs). Every call's future
    /// is Send-wrapped: the wrapper's inner carries all the !Send
    /// js state, the outer promise stays honest.
    #[derive(Clone)]
    pub struct GrpcWebSession(
        SendWrapper<MailboxServiceClient<tonic_web_wasm_client::Client>>,
        /// The claim-issued session credential, header-ready. `None`
        /// until the claim succeeds; a superseding claim retires the
        /// older token (re-claim, never cache).
        Option<String>,
    );

    /// The plane stream: the js-backed `tonic::Streaming` behind the
    /// Send bridge (Streaming is Unpin over its boxed inner), error-
    /// mapped to the transport's neutral Error.
    pub type GrpcWebPlanes = futures::stream::Map<
        SendStream<tonic::Streaming<relay::PlaneFrame>>,
        fn(core::result::Result<relay::PlaneFrame, tonic::Status>) -> Result<relay::PlaneFrame>,
    >;

    fn plane_item(
        item: core::result::Result<relay::PlaneFrame, tonic::Status>,
    ) -> Result<relay::PlaneFrame> {
        item.map_err(|status| Error::Call {
            message: status.to_string(),
        })
    }

    /// The Stream half of the bridge: send_wrapper only bridges
    /// Future; this newtype adds Stream (delegating poll_next to the
    /// Unpin inner through the wrapper).
    pub struct SendStream<S>(SendWrapper<S>);

    impl<S> Stream for SendStream<S>
    where
        S: Stream + Unpin,
    {
        type Item = S::Item;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<<S as Stream>::Item>> {
            std::pin::Pin::new(&mut self.0).poll_next(cx)
        }
    }

    // SendWrapper is unconditionally Unpin (a boxed inner), so the
    // poll delegation above can pin the wrapper freely.
    impl<S> Unpin for SendStream<S> {}

    fn st(status: tonic::Status) -> Error {
        Error::Call {
            message: status.to_string(),
        }
    }

    // TEMP diagnostic (retire list pending): the visit call ladder,
    // TIMESTAMPED (106's claim-latency ask, window-receipt verdict §6):
    // every line carries the ms since the visit's first traced call
    // (cumulative) and the ms since the previous line (the leg delta).
    // The decomposed legs = get_version/nonce-fetch → register →
    // challenge → claim → set_spk/set_opks → each plane OPEN; the
    // widest leg names the dominant tunnel cost (the relay's claim
    // RPC itself is single-digit ms — 106's analysis). The clock =
    // js_sys::Date::now (wall millis; the deltas are seconds-scale —
    // NTP jumps are noise here, not a monotonicity requirement).
    static VIS_T0: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static LAST_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn wall_ms() -> u64 {
        js_sys::Date::now().max(0.0) as u64
    }

    fn trace(s: &str) {
        let now = wall_ms();
        let t0 = VIS_T0.load(std::sync::atomic::Ordering::Relaxed);
        let last = LAST_MS.swap(now, std::sync::atomic::Ordering::Relaxed);
        let (cum, leg) = if t0 == 0 || now < t0 || now < last {
            (0, 0)
        } else {
            (now - t0, now - last)
        };
        web_sys::console::log_1(&format!("[carrier] +{cum}ms (+{leg}ms) {s}").into());
    }

    /// The ladder's head resets the visit clock (get_version is the
    /// ceremony's first call every visit).
    fn trace_visit_start(s: &str) {
        let now = wall_ms();
        VIS_T0.store(now, std::sync::atomic::Ordering::Relaxed);
        LAST_MS.store(now, std::sync::atomic::Ordering::Relaxed);
        web_sys::console::log_1(&format!("[carrier] +0ms (+0ms) {s}").into());
    }

    impl MailSession for GrpcWebSession {
        type Planes = GrpcWebPlanes;

        async fn get_version(&mut self) -> Result<u32> {
            trace_visit_start("get_version enter");
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    Ok(c.get_version(relay::GetVersionRequest {})
                        .await
                        .map_err(st)?
                        .into_inner()
                        .version)
                })
                .await
            })()
            .await;
            trace(&format!("get_version exit: {r:?}"));
            r
        }

        async fn register_mailbox(&mut self, mailbox: mail::MailboxPublicSet) -> Result<()> {
            trace("register_mailbox enter");
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    c.register_mailbox(relay::RegisterMailboxRequest {
                        mailbox: Some(mailbox),
                    })
                    .await
                    .map_err(st)?;
                    Ok(())
                })
                .await
            })()
            .await;
            trace("register_mailbox exit");
            r
        }

        async fn get_challenge(&mut self) -> Result<relay::Challenge> {
            trace("get_challenge enter");
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    Ok(c.get_challenge(relay::GetChallengeRequest {})
                        .await
                        .map_err(st)?
                        .into_inner())
                })
                .await
            })()
            .await;
            trace("get_challenge exit");
            r
        }

        async fn claim_mailbox(&mut self, anchor: Vec<u8>, claim_sig: Vec<u8>) -> Result<()> {
            trace("claim_mailbox enter");
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    let response = c
                        .claim_mailbox(relay::ClaimMailboxRequest { anchor, claim_sig })
                        .await
                        .map_err(st)?;
                    let token = response
                        .metadata()
                        .get(SESSION_TOKEN_HEADER)
                        .ok_or(Error::Call {
                            message: "the claim reply carries no session token".into(),
                        })?
                        .to_str()
                        .map_err(|_| Error::Call {
                            message: "the session token header is not ASCII".into(),
                        })?
                        .to_owned();
                    trace("claim_mailbox: session token acquired");
                    Ok(token)
                })
                .await
            })()
            .await;
            trace(&format!("claim_mailbox exit: {r:?}"));
            r.map(|token| {
                self.1 = Some(token);
            })
        }

        async fn set_spk(&mut self, spk: mail::Spk) -> Result<()> {
            trace("set_spk enter");
            let token = self.1.clone();
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    c.set_spk(present(&token, relay::SetSpkRequest { spk: Some(spk) }))
                        .await
                        .map_err(st)?;
                    Ok(())
                })
                .await
            })()
            .await;
            trace("set_spk exit");
            r
        }

        async fn set_opks(&mut self, opks: Vec<mail::Opk>) -> Result<()> {
            trace("set_opks enter");
            let token = self.1.clone();
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    c.set_opks(present(&token, relay::SetOpksRequest { opks }))
                        .await
                        .map_err(st)?;
                    Ok(())
                })
                .await
            })()
            .await;
            trace("set_opks exit");
            r
        }

        async fn get_bundle(&mut self, mailbox: Vec<u8>) -> Result<relay::Bundle> {
            trace("get_bundle enter");
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    Ok(c.get_bundle(relay::GetBundleRequest { mailbox })
                        .await
                        .map_err(st)?
                        .into_inner())
                })
                .await
            })()
            .await;
            trace(&format!("get_bundle exit: {r:?}"));
            r
        }

        async fn send_transcript_mail(&mut self, mail: mail::Mail) -> Result<relay::MailReceipt> {
            trace("send_transcript_mail enter");
            let token = self.1.clone();
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    Ok(c.send_transcript_mail(present(&token, mail))
                        .await
                        .map_err(st)?
                        .into_inner())
                })
                .await
            })()
            .await;
            trace(&format!("send_transcript_mail exit: {r:?}"));
            r
        }

        async fn send_resources_mail(&mut self, mail: mail::Mail) -> Result<relay::MailReceipt> {
            trace("send_resources_mail enter");
            let token = self.1.clone();
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    Ok(c.send_resources_mail(present(&token, mail))
                        .await
                        .map_err(st)?
                        .into_inner())
                })
                .await
            })()
            .await;
            trace(&format!("send_resources_mail exit: {r:?}"));
            r
        }

        async fn send_signature_mail(&mut self, mail: mail::Mail) -> Result<relay::MailReceipt> {
            trace("send_signature_mail enter");
            let token = self.1.clone();
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    Ok(c.send_signature_mail(present(&token, mail))
                        .await
                        .map_err(st)?
                        .into_inner())
                })
                .await
            })()
            .await;
            trace(&format!("send_signature_mail exit: {r:?}"));
            r
        }

        async fn report_mail_received(
            &mut self,
            from_mailbox: Vec<u8>,
            up_to: u64,
            key_version: u32,
        ) -> Result<()> {
            trace("report_mail_received enter");
            let token = self.1.clone();
            let r = (|| async {
                let client = self.0.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    c.report_mail_received(present(
                        &token,
                        relay::ReportMailReceivedRequest {
                            from_mailbox,
                            up_to,
                            key_version,
                        },
                    ))
                    .await
                    .map_err(st)?;
                    Ok(())
                })
                .await
            })()
            .await;
            trace(&format!("report_mail_received exit: {r:?}"));
            r
        }

        async fn get_mail_cursors(&mut self) -> Result<Vec<relay::MailboxCursor>> {
            let token = self.1.clone();
            let client = self.0.clone();
            SendWrapper::new(async move {
                let mut c = client;
                Ok(
                    c.get_mail_cursors(present(&token, relay::GetMailCursorsRequest {}))
                        .await
                        .map_err(st)?
                        .into_inner()
                        .outbox,
                )
            })
            .await
        }

        async fn transcript_plane(&mut self) -> Result<Self::Planes> {
            trace("transcript_plane enter");
            let token = self.1.clone();
            let client = self.0.clone();
            let r = open_plane_rpc(move || {
                let token = token.clone();
                let client = client.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    let stream = c
                        .transcript_plane(present(&token, relay::TranscriptPlaneRequest {}))
                        .await
                        .map_err(st)?
                        .into_inner();
                    Ok(SendStream(SendWrapper::new(stream)).map(plane_item as _))
                })
            })
            .await;
            match &r {
                Ok(_) => trace("transcript_plane exit: ok"),
                Err(e) => trace(&format!("transcript_plane exit: {e}")),
            }
            r
        }

        async fn resources_plane(&mut self) -> Result<Self::Planes> {
            trace("resources_plane enter");
            let token = self.1.clone();
            let client = self.0.clone();
            let r = open_plane_rpc(move || {
                let token = token.clone();
                let client = client.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    let stream = c
                        .resources_plane(present(&token, relay::ResourcesPlaneRequest {}))
                        .await
                        .map_err(st)?
                        .into_inner();
                    Ok(SendStream(SendWrapper::new(stream)).map(plane_item as _))
                })
            })
            .await;
            match &r {
                Ok(_) => trace("resources_plane exit: ok"),
                Err(e) => trace(&format!("resources_plane exit: {e}")),
            }
            r
        }

        async fn signature_plane(&mut self) -> Result<Self::Planes> {
            trace("signature_plane enter");
            let token = self.1.clone();
            let client = self.0.clone();
            let r = open_plane_rpc(move || {
                let token = token.clone();
                let client = client.clone();
                SendWrapper::new(async move {
                    let mut c = client;
                    let stream = c
                        .signature_plane(present(&token, relay::SignaturePlaneRequest {}))
                        .await
                        .map_err(st)?
                        .into_inner();
                    Ok(SendStream(SendWrapper::new(stream)).map(plane_item as _))
                })
            })
            .await;
            match &r {
                Ok(_) => trace("signature_plane exit: ok"),
                Err(e) => trace(&format!("signature_plane exit: {e}")),
            }
            r
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use imp::{GrpcWebCarrier, GrpcWebPlanes, GrpcWebSession};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    // The native shell: never runs (the core lives in a browser
    // worker, A17); the workspace's host-target `cargo check` still
    // needs the full trait surface to resolve.
    use zconned::transport::{Carrier, Error, MailSession, Result};
    use zconnor_mail_types::{mail, relay};

    pub struct GrpcWebCarrier;

    impl GrpcWebCarrier {
        pub fn new(_endpoint: impl Into<String>) -> Self {
            Self
        }
    }

    #[derive(Clone)]
    pub struct GrpcWebSession;

    fn never<T>() -> Result<T> {
        Err(Error::Call {
            message: "the native shell opens no session".into(),
        })
    }

    impl Carrier for GrpcWebCarrier {
        type Session = GrpcWebSession;

        async fn open(&self) -> Result<Self::Session> {
            never()
        }
    }

    impl MailSession for GrpcWebSession {
        type Planes = futures::stream::Empty<Result<relay::PlaneFrame>>;

        async fn get_version(&mut self) -> Result<u32> {
            never()
        }
        async fn register_mailbox(&mut self, _mailbox: mail::MailboxPublicSet) -> Result<()> {
            never()
        }
        async fn get_challenge(&mut self) -> Result<relay::Challenge> {
            never()
        }
        async fn claim_mailbox(&mut self, _anchor: Vec<u8>, _claim_sig: Vec<u8>) -> Result<()> {
            never()
        }
        async fn set_spk(&mut self, _spk: mail::Spk) -> Result<()> {
            never()
        }
        async fn set_opks(&mut self, _opks: Vec<mail::Opk>) -> Result<()> {
            never()
        }
        async fn get_bundle(&mut self, _mailbox: Vec<u8>) -> Result<relay::Bundle> {
            never()
        }
        async fn send_transcript_mail(&mut self, _mail: mail::Mail) -> Result<relay::MailReceipt> {
            never()
        }
        async fn send_resources_mail(&mut self, _mail: mail::Mail) -> Result<relay::MailReceipt> {
            never()
        }
        async fn send_signature_mail(&mut self, _mail: mail::Mail) -> Result<relay::MailReceipt> {
            never()
        }
        async fn report_mail_received(
            &mut self,
            _from_mailbox: Vec<u8>,
            _up_to: u64,
            _key_version: u32,
        ) -> Result<()> {
            never()
        }
        async fn get_mail_cursors(&mut self) -> Result<Vec<relay::MailboxCursor>> {
            never()
        }
        async fn transcript_plane(&mut self) -> Result<Self::Planes> {
            never()
        }
        async fn resources_plane(&mut self) -> Result<Self::Planes> {
            never()
        }
        async fn signature_plane(&mut self) -> Result<Self::Planes> {
            never()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use imp::GrpcWebCarrier;
#[cfg(not(target_arch = "wasm32"))]
pub use imp::GrpcWebSession;
