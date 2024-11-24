// We only need `startup` here which is the main entry point
// In theory, we could also use all other functions/struct types from Rust which we have bound with
// `#[wasm_bindgen]`
import init, { startup, initThreadPool } from "./pkg/webworker.js";

async function run_wasm() {
  // Load the Wasm file by awaiting the Promise returned by `wasm_bindgen`
  // `wasm_bindgen` was imported in `index.html`
  await init();

  // Thread pool initialization with the given number of threads
  // (pass `navigator.hardwareConcurrency` if you want to use all cores).
  await initThreadPool(navigator.hardwareConcurrency);

  console.log("index.js loaded");

  // Run main Wasm entry point
  // This will create a worker from within our Rust code compiled to Wasm
  startup();
}

run_wasm();
