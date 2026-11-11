"serviceWorker"in navigator&&window.addEventListener("load",()=>{navigator.serviceWorker.register("./sw.js").catch(e=>{console.warn("sw registration failed",e)})});
