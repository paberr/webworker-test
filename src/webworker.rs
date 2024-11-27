use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use js_sys::{
    wasm_bindgen::{prelude::Closure, JsCast, JsValue, UnwrapThrowExt},
    Array,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Semaphore};
use web_sys::{
    window, Blob, BlobPropertyBag, MessageEvent, Url, Worker, WorkerOptions, WorkerType,
};

use crate::{
    error::{Error, TryRunError},
    func::WebWorkerFn,
};

pub type Callback = dyn FnMut(MessageEvent);
pub const WORKER_JS: &str = r#"
import init, * as funcs from "{{wasm}}";
console.debug('Initializing worker');

(async () => {
    await init();
    self.postMessage('post-init');

    self.addEventListener('message', async event => {
        console.debug('Received worker event');
        const { id, func_name, arg } = event.data;

        const fn = funcs[func_name];
        if (!fn) {
            console.error(`Function '${func_name}' is not exported.`);
            self.postMessage({ id: id, response: null });
            return;
        }

        const worker_result = await fn(arg);

        // Send response back to be handled by callback in main thread.
        console.debug('Send worker result');
        self.postMessage({ id: id, response: worker_result });
    });
})();
"#;

#[derive(Serialize, Deserialize)]
struct Request<'a> {
    id: usize,
    func_name: &'static str,
    #[serde(with = "serde_bytes")]
    arg: &'a [u8],
}

#[derive(Serialize, Deserialize)]
struct Response {
    id: usize,
    #[serde(with = "serde_bytes")]
    response: Option<Vec<u8>>,
}

pub struct WebWorker {
    worker: Worker,
    task_limit: Option<Semaphore>,
    current_task: AtomicUsize,
    open_tasks: Arc<RwLock<HashMap<usize, oneshot::Sender<Response>>>>,
    _callback: Closure<Callback>,
}

impl WebWorker {
    fn worker_blob(wasm_path: &str) -> Result<String, JsValue> {
        let blob_options = BlobPropertyBag::new();
        blob_options.set_type("application/javascript");

        let origin = window()
            .ok_or_else(|| JsValue::from("window missing"))?
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
    pub async fn new(main_js: &str, task_limit: Option<usize>) -> Result<WebWorker, JsValue> {
        // Create worker
        let worker_options = WorkerOptions::new();
        worker_options.set_type(WorkerType::Module);
        let worker = Worker::new_with_options(&WebWorker::worker_blob(main_js)?, &worker_options)?;

        // Wait until worker is initialized.
        let (tx, rx) = oneshot::channel();
        let handler = Closure::once(move |_: MessageEvent| {
            let _ = tx.send(());
        });
        worker.set_onmessage(Some(handler.as_ref().unchecked_ref()));
        rx.await.expect_throw("Webworker init sender dropped");

        let tasks = Arc::new(RwLock::new(HashMap::new()));

        let callback_handle = Self::callback(Arc::clone(&tasks));
        worker.set_onmessage(Some(callback_handle.as_ref().unchecked_ref()));

        Ok(WebWorker {
            worker,
            task_limit: task_limit.map(|limit| Semaphore::new(limit)),
            current_task: AtomicUsize::new(0),
            open_tasks: tasks,
            _callback: callback_handle,
        })
    }

    /// Function to be called when a result is ready.
    fn callback(
        tasks: Arc<RwLock<HashMap<usize, oneshot::Sender<Response>>>>,
    ) -> Closure<dyn FnMut(MessageEvent)> {
        Closure::new(move |event: MessageEvent| {
            let data = event.data();
            let response: Response =
                serde_wasm_bindgen::from_value(data).expect_throw("Could not deserialize response");
            let mut tasks_wg = tasks.write();

            // Send response on channel.
            if let Some(channel) = tasks_wg.remove(&response.id) {
                // Ignore if receiver is already closed.
                let _ = channel.send(response);
            }
        })
    }

    pub async fn run(&self, func: WebWorkerFn, arg: &[u8]) -> Result<Vec<u8>, Error> {
        // Acquire permit if necessary.
        let _permit = if let Some(ref s) = self.task_limit {
            Some(s.acquire().await.unwrap())
        } else {
            None
        };

        self.force_run(func, arg).await
    }

    pub async fn try_run(&self, func: WebWorkerFn, arg: &[u8]) -> Result<Vec<u8>, TryRunError> {
        // Try-acquire permit if necessary.
        let _permit = if let Some(ref s) = self.task_limit {
            Some(match s.try_acquire() {
                Ok(permit) => permit,
                Err(_) => return Err(TryRunError::Full),
            })
        } else {
            None
        };

        Ok(self.force_run(func, arg).await?)
    }

    async fn force_run(&self, func: WebWorkerFn, arg: &[u8]) -> Result<Vec<u8>, Error> {
        let id = self.current_task.fetch_add(1, Ordering::AcqRel);
        let request = Request {
            id,
            func_name: func.name,
            arg,
        };

        // Create channel and add task.
        let (sender, receiver) = oneshot::channel();
        self.open_tasks.write().insert(id, sender);

        self.worker
            .post_message(
                &serde_wasm_bindgen::to_value(&request).expect_throw("Could not serialize request"),
            )
            .map_err(|_| Error::WorkerLost)?;

        // Handle result.
        match receiver.await {
            // Success case:
            Ok(Response {
                response: Some(result),
                ..
            }) => Ok(result),
            // Function not found:
            Ok(Response { response: None, .. }) => Err(Error::FnNotFound(func.name)),
            Err(_) => Err(Error::WorkerLost),
        }
    }
}
