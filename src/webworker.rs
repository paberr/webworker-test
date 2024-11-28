use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use js_sys::{
    wasm_bindgen::{self, prelude::Closure, JsCast, JsValue, UnwrapThrowExt},
    Array, JsString,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Semaphore};
use web_sys::{Blob, BlobPropertyBag, MessageEvent, Url, Worker, WorkerOptions, WorkerType};

use crate::{
    convert::{from_bytes, to_bytes},
    error::Full,
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

        const webworker_func_name = `__webworker_${func_name}`;
        const fn = funcs[webworker_func_name];
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
struct Request {
    id: usize,
    func_name: &'static str,
    #[serde(with = "serde_bytes")]
    arg: Box<[u8]>,
}

#[derive(Serialize, Deserialize)]
struct Response {
    id: usize,
    #[serde(with = "serde_bytes")]
    response: Option<Vec<u8>>,
}

pub fn main_js() -> JsString {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(thread_local, js_namespace = ["import", "meta"], js_name = url)]
        static URL: JsString;
    }

    URL.with(Clone::clone)
}

pub struct WebWorker {
    worker: Worker,
    task_limit: Option<Semaphore>,
    current_task: Cell<usize>,
    open_tasks: Rc<RefCell<HashMap<usize, oneshot::Sender<Response>>>>,
    _callback: Closure<Callback>,
}

impl WebWorker {
    fn worker_blob(wasm_path: Option<&str>) -> String {
        let blob_options = BlobPropertyBag::new();
        blob_options.set_type("application/javascript");

        let mut wasm_path_owned = None;
        let wasm_path = wasm_path.unwrap_or_else(|| {
            // TODO: Make nicer
            wasm_path_owned = Some(main_js().as_string().unwrap_throw());
            &wasm_path_owned.as_ref().unwrap_throw()
        });

        let code = Array::new();
        code.push(&JsValue::from_str(
            &WORKER_JS.replace("{{wasm}}", wasm_path),
        ));

        Url::create_object_url_with_blob(
            &Blob::new_with_blob_sequence_and_options(&code.into(), &blob_options)
                .expect_throw("Couldn't create blob"),
        )
        .expect_throw("Couldn't create object URL")
    }

    pub async fn new(task_limit: Option<usize>) -> WebWorker {
        Self::with_path(None, task_limit).await
    }

    /// Create a new WrappedWorker
    pub async fn with_path(main_js: Option<&str>, task_limit: Option<usize>) -> WebWorker {
        // Create worker
        let worker_options = WorkerOptions::new();
        worker_options.set_type(WorkerType::Module);
        let script_url = WebWorker::worker_blob(main_js);
        let worker = Worker::new_with_options(&script_url, &worker_options)
            .expect_throw("Couldn't create worker");

        // Wait until worker is initialized.
        let (tx, rx) = oneshot::channel();
        let handler = Closure::once(move |_: MessageEvent| {
            let _ = tx.send(());
        });
        worker.set_onmessage(Some(handler.as_ref().unchecked_ref()));
        rx.await.expect_throw("Webworker init sender dropped");

        let tasks = Rc::new(RefCell::new(HashMap::new()));

        let callback_handle = Self::callback(Rc::clone(&tasks));
        worker.set_onmessage(Some(callback_handle.as_ref().unchecked_ref()));

        WebWorker {
            worker,
            task_limit: task_limit.map(|limit| Semaphore::new(limit)),
            current_task: Cell::new(0),
            open_tasks: tasks,
            _callback: callback_handle,
        }
    }

    /// Function to be called when a result is ready.
    fn callback(
        tasks: Rc<RefCell<HashMap<usize, oneshot::Sender<Response>>>>,
    ) -> Closure<dyn FnMut(MessageEvent)> {
        Closure::new(move |event: MessageEvent| {
            let data = event.data();
            let response: Response =
                serde_wasm_bindgen::from_value(data).expect_throw("Could not deserialize response");
            let mut tasks_wg = tasks.borrow_mut();

            // Send response on channel.
            if let Some(channel) = tasks_wg.remove(&response.id) {
                // Ignore if receiver is already closed.
                let _ = channel.send(response);
            }
        })
    }

    #[cfg(feature = "serde")]
    pub async fn run<T, R>(&self, func: WebWorkerFn<T, R>, arg: &T) -> R
    where
        T: Serialize + for<'de> Deserialize<'de>,
        R: Serialize + for<'de> Deserialize<'de>,
    {
        // Acquire permit if necessary.
        let _permit = if let Some(ref s) = self.task_limit {
            Some(s.acquire().await.unwrap())
        } else {
            None
        };

        // Convert arg and result.
        self.force_run(func.name, arg).await
    }

    #[cfg(feature = "serde")]
    pub async fn try_run<T, R>(&self, func: WebWorkerFn<T, R>, arg: &T) -> Result<R, Full>
    where
        T: Serialize + for<'de> Deserialize<'de>,
        R: Serialize + for<'de> Deserialize<'de>,
    {
        // Try-acquire permit if necessary.
        let _permit = if let Some(ref s) = self.task_limit {
            Some(match s.try_acquire() {
                Ok(permit) => permit,
                Err(_) => return Err(Full),
            })
        } else {
            None
        };

        // Convert arg and result.
        Ok(self.force_run(func.name, arg).await)
    }

    pub async fn run_bytes(
        &self,
        func: WebWorkerFn<Box<[u8]>, Box<[u8]>>,
        arg: &Box<[u8]>,
    ) -> Box<[u8]> {
        // Acquire permit if necessary.
        let _permit = if let Some(ref s) = self.task_limit {
            Some(s.acquire().await.unwrap())
        } else {
            None
        };

        self.force_run(func.name, arg).await
    }

    pub async fn try_run_bytes(
        &self,
        func: WebWorkerFn<Box<[u8]>, Box<[u8]>>,
        arg: &Box<[u8]>,
    ) -> Result<Box<u8>, Full> {
        // Try-acquire permit if necessary.
        let _permit = if let Some(ref s) = self.task_limit {
            Some(match s.try_acquire() {
                Ok(permit) => permit,
                Err(_) => return Err(Full),
            })
        } else {
            None
        };

        Ok(self.force_run(func.name, arg).await)
    }

    async fn force_run<T, R>(&self, func_name: &'static str, arg: &T) -> R
    where
        T: Serialize + for<'de> Deserialize<'de>,
        R: Serialize + for<'de> Deserialize<'de>,
    {
        let id = self.current_task.get();
        self.current_task.set(id.wrapping_add(1));
        let request = Request {
            id,
            func_name,
            arg: to_bytes(arg),
        };

        // Create channel and add task.
        let (sender, receiver) = oneshot::channel();
        self.open_tasks.borrow_mut().insert(id, sender);

        self.worker
            .post_message(
                &serde_wasm_bindgen::to_value(&request).expect_throw("Could not serialize request"),
            )
            .expect_throw("WebWorker gone");

        // Handle result.
        let res = receiver
            .await
            .expect_throw("WebWorker gone")
            .response
            .expect_throw("Could not find function");
        from_bytes(&res)
    }

    /// Return the current capacity for new tasks.
    pub fn capacity(&self) -> Option<usize> {
        self.task_limit.as_ref().map(|s| s.available_permits())
    }

    /// Return the number of tasks currently queued to this worker.
    pub fn current_load(&self) -> usize {
        self.open_tasks.borrow().len()
    }
}
