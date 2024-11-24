// The worker has its own scope and no direct access to functions/objects of the
// global scope. We import the generated JS file to make `wasm_bindgen`
// available which we need to initialize our Wasm code.
import init, * as funcs from "./pkg/webworker.js";

console.log('Initializing worker')

// In the worker, we have a different struct that we want to use as in
// `index.js`.

async function init_wasm_in_worker() {
    // Load the Wasm file by awaiting the Promise returned by `wasm_bindgen`.
    // await init('./pkg/webworker_bg.wasm');
    await init();

    // Set callback to handle messages passed to the worker.
    self.onmessage = async event => {
        console.log('Receive worker result');
        // By using methods of a struct as reaction to messages passed to the
        // worker, we can preserve our state between messages.
        var worker_result = funcs.uncompress_validators(event.data);

        // Send response back to be handled by callback in main thread.
        console.log('Send worker result');
        self.postMessage(worker_result);
    };
};

init_wasm_in_worker();
