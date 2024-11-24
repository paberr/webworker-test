use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use js_sys::Array;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{console, window, Blob, MessageEvent, Url, Worker, WorkerOptions, WorkerType};

pub type Callback = dyn FnMut(MessageEvent);
pub const WORKER_JS: &str = r#"
import init, * as funcs from "{{wasm}}";
console.debug('Initializing worker');

(async () => {
    await init();

    self.onmessage = async event => {
        console.trace('Received worker event');
        const { id, func_name, arg } = event.data;

        const fn = funcs[func_name];
        if (!fn) return console.error(`Couldn't find function '${func_name}', make sure it is exported.`);

        const worker_result = await fn(arg);

        // Send response back to be handled by callback in main thread.
        console.log('Send worker result');
        self.postMessage({ id: id, result: worker_result });
    });
})();
"#;

#[derive(Serialize, Deserialize)]
struct Request {
    id: usize,
    func_name: String,
    #[serde(with = "serde_bytes")]
    arg: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Response {
    id: usize,
    #[serde(with = "serde_bytes")]
    response: Vec<u8>,
}

pub struct WrappedWorker {
    worker: Worker,
    task_limit: Option<usize>,
    current_task: AtomicUsize,
    open_tasks: Arc<RwLock<HashMap<usize, oneshot::Sender<Vec<u8>>>>>,
    callback: Closure<Callback>,
}

impl WrappedWorker {
    fn worker_blob(wasm_path: &str) -> Result<String, JsValue> {
        let blob_options = BlobPropertyBag::new();
        blob_options.set_type("application/javascript");

        let origin = window()
            .ok_or_else(|| "window missing".into())?
            .location()
            .origin()?
            .to_string();

        let code = Array::new();
        code.push(&JsValue::from_str(
            &WORKER_JS.replace("{{wasm}}", &format!("{}{}", origin, wasm_path)),
        ));

        Url::create_object_url_with_blob(&Blob::new_with_blob_sequence_and_options(
            &code.into(),
            &blob_options,
        )?)
    }

    /// Create a new WrappedWorker
    pub(crate) fn new(main_js: &str, task_limit: Option<usize>) -> Result<WrappedWorker, JsValue> {
        // Create worker
        let worker_options = WorkerOptions::new();
        worker_options.set_type(WorkerType::Module);
        let worker =
            Worker::new_with_options(&WrappedWorker::worker_blob(main_js)?, &worker_options)?;

        let tasks = Arc::new(RwLock::new(HashMap::new()));

        let callback_handle = Self::callback(Arc::clone(&tasks));
        worker.set_onmessage(Some(callback_handle.as_ref().unchecked_ref()));

        Ok(WrappedWorker {
            worker,
            task_limit,
            current_task: AtomicUsize::new(0),
            open_tasks: tasks,
            callback: callback_handle,
        })
    }

    /// Callback
    fn callback(
        tasks: Arc<RwLock<HashMap<usize, oneshot::Sender<Vec<u8>>>>>,
    ) -> Closure<dyn FnMut(MessageEvent)> {
        Closure::new(move |event: MessageEvent| {
            let data = event.data();
            let response: Response =
                serde_wasm_bindgen::from_value(data).expect("Couldn't deserialize response");
            let mut tasks_wg = tasks.write();

            // Send response on channel.
            if let Some(channel) = tasks_wg.remove(&response.id) {
                // Ignore if receiver is already closed.
                let _ = channel.send(response.response);
            }
        })
    }

    pub async fn run(&self, func_name: String, arg: Vec<u8>) -> Result<Vec<u8>, JsValue> {
        let id = self.current_task.fetch_add(1, Ordering::AcqRel);
        let request = Request { id, func_name, arg };

        // Create channel and add task.
        let (sender, receiver) = oneshot::channel();
        self.open_tasks.write().insert(id, sender);

        self.worker
            .post_message(&serde_wasm_bindgen::to_value(&request).unwrap())
            .expect("Failed to post message");

        // Handle result.
        match receiver.await {
            Ok(result) => Ok(result),
            Err(_) => todo!(),
        }
    }
}
