/* tslint:disable */
/* eslint-disable */

/**
 * The worker's entry point (called by the JS glue once wasm is up).
 */
export function tafy_worker_main(): void;

/**
 * The storage face's self-test (usable before the core's pump ask
 * lands in 106): open the store, write, append, rename, exists,
 * read back, read dirs — then leave the results as console logs.
 */
export function tafy_worker_storage_selftest(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly tafy_worker_main: () => void;
    readonly tafy_worker_storage_selftest: () => void;
    readonly wasm_bindgen_f46a8e7005748a90___convert__closures_____invoke___js_sys_8d224ca59892f7fb___Function_fn_wasm_bindgen_f46a8e7005748a90___JsValue_____wasm_bindgen_f46a8e7005748a90___sys__Undefined___js_sys_8d224ca59892f7fb___Function_fn_wasm_bindgen_f46a8e7005748a90___JsValue_____wasm_bindgen_f46a8e7005748a90___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_f46a8e7005748a90___convert__closures_____invoke___wasm_bindgen_f46a8e7005748a90___JsValue__core_ed718c3d60ebd546___result__Result_____wasm_bindgen_f46a8e7005748a90___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_f46a8e7005748a90___convert__closures_____invoke___web_sys_8903779cb7957908___features__gen_MessageEvent__MessageEvent______true_: (a: number, b: number, c: any) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
