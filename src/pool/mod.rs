use futures::future::join_all;
use js_sys::wasm_bindgen::UnwrapThrowExt;
use scheduler::Scheduler;
pub use scheduler::Strategy;
use web_sys::window;

use crate::WebWorker;

mod scheduler;

pub struct WebWorkerPool {
    workers: Vec<WebWorker>,
    scheduler: Scheduler,
}

impl WebWorkerPool {
    pub async fn new() -> Self {
        Self::with_strategy(Strategy::RoundRobin).await
    }

    pub async fn with_strategy(strategy: Strategy) -> Self {
        let num_workers = window()
            .expect_throw("Window missing")
            .navigator()
            .hardware_concurrency() as usize;
        Self::with_options(strategy, num_workers).await
    }

    pub async fn with_num_workers(num_workers: usize) -> Self {
        Self::with_options(Strategy::RoundRobin, num_workers).await
    }

    pub async fn with_options(strategy: Strategy, num_workers: usize) -> Self {
        let worker_inits = (0..num_workers).map(|_| {
            // Do not impose a task limit.
            WebWorker::new(None)
        });
        let workers = join_all(worker_inits).await;

        Self {
            workers,
            scheduler: Scheduler::new(strategy),
        }
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
}
