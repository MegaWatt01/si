use crate::lifeguard::LifeGuard;
use crate::task::{PoolNoodleTask, PoolNoodleTaskType};
use crossbeam_queue::ArrayQueue;
use std::fmt::Display;
use std::result;
use std::sync::Arc;
use telemetry_utils::metric;
use tokio::time::{self, sleep};
use tokio_util::sync::CancellationToken;

use tokio::time::Duration;
use tracing::{debug, info, warn};

use crate::errors::PoolNoodleError;
use crate::{Instance, Spec};

/// [`pool_noodle`] implementations.

///---------------------------------------------------------------------
///---------------------------------------------------------------------
///---------------------------:::::::::::::::::::::---------------------
///---------------------:::::::::::::::::::::-------::::::--------------
///-----------------:::::::::::::::::::==========------::::::-----------
///----#*+#-----::::::::::::::::::::::::---:::--==++++-:::::::::--------
///---+#%@@#----::::::::::::::::::===========++++***#*=::::::::::::-----
///--=+*@@@@@*::::::::::::::::::::========++++****###*=::::::::::::::---
///--=@@@@@@@@%+::::::::::::::::::-=======+++++**###%#+:::::::::::::::--
///----=@@@@@@@@%=:::::::::::::::::=========++++*###%#+-::::::::::::::::
///------#@@@@@@@@%-:::::::::::::::-....:===:....:*#%#+-::::::::::::::::
///-----:::#@@@@@@@@%-::::::::::::..:-=.::+.:=**:-==%#*=::::::::::::::::
///----::::::%@@@@@@@@%=::::::::::.-#%@@--=.#%@@#*#*%%*=::::::::::::::::
///---:::::::::@@@@@@@@@%=::::::..:-*@@***#+-+####*%%%#+::::::::::::::::
///-:::::::::::%@@@@@@@@@@%#:.......+***#%%###**#%%%%%#+::::::::::::::::
///:::::::::::*#**#@@@@@@@@@%:......=+*#%%%%%@%#%%%%%%#*-:::::::::::::::
///::::::::::#@*+**%@@@@@@@@@=......:++*******####%%%%%*=:::::::::::::::
///:::::::::::=%##%@@@#%@%@@+........++++******###%%%%%*=:::::::::::::::
///:::::::::::=+*#@@%#%%%%@*.........++++++*****###%%%%#+.::::::::::::::
///::::::::::=++===+*%*===+=.........++++++*****####%%%#**=:::::::::::::
///:::::::::===++++*#@*+++**=........-+++++******###%%%%#%###*-:::::::::
///::::::::-++===*%@@@@%%%##:........++++++******###%%%%%%%###**+:::::::
///:::::::::*%***%@@@@@%%%#:.......=+=+++++******####%%%%#%###****+:::::
///::::::::::*@@@@@@@%%%%%#......-+==+++*********####%%%%+:.=##****+-:::
///:::::::::::::%@@@-%%%%%#*:.:++++++************####%%%%+::::-***+**-::
///--::::::::::::::::-#%%%###********#-**********####%%%#*.:::::**++**-:
///--::::::::::::::::::*#%%%#######*:..**********####%%%#*.:::::=*++**=:
///---:::::::::::::::::::-#%%###*-.....**********#####%%##::::::=*++**+:
///---::::::::::::::::::::::::::.......+*********#####%%%#-:::::+++**#=:
///---::::::::::::::::::::::::::::.....-********######%%%#+::::*++**##::
///----:::::::::::::::::::::::::::::::::#*******######%%%#*****++**##=::
///------::::::::::::::::::::::::::::::.********######%%%%##**+***#%=:::
///---------::::::::::::::::::::::::::::********######%%%%%*****##%-::::
///---------::::::::::::::::::::::::::::+*******######%%%%%#**##%=:::---
///----------:::::::::::::::::::::::::::-#*****#######%%%%%-:+#=::::----
///:------------:::::::::::::::::::::::::##****#######%%%%%+:::::::-----
///=:----------::::::::::::::::::::::::::*#***########%%%%%*::::::------

type Result<T, E> = result::Result<T, PoolNoodleError<E>>;

#[derive(Clone, Debug)]
/// Configuration object for setting up pool noodle
pub struct PoolNoodleConfig<S> {
    /// Verify instances can be started and stopped before starting the pool management tasks
    pub check_health: bool,
    /// Max number of worker threads to run at once. Defaults to available_parallelism() or 16
    pub max_concurrency: u32,
    /// Maximum number of instances to manage at once
    pub pool_size: u32,
    /// Number of attempts to get from the pool before giving up with 10 ms between attempts
    pub retry_limit: u32,
    /// Shuts down the pool management tasks
    pub shutdown_token: CancellationToken,
    /// The spec for the type of instance to manage
    pub spec: S,
}

impl<S> Default for PoolNoodleConfig<S>
where
    S: Spec + Default,
{
    fn default() -> Self {
        let concurrency = {
            match std::thread::available_parallelism() {
                Ok(p) => p.get() as u32,
                Err(_) => 16,
            }
        };
        Self {
            check_health: false,
            max_concurrency: concurrency,
            pool_size: 100,
            retry_limit: 6000,
            shutdown_token: CancellationToken::new(),
            spec: S::default(),
        }
    }
}

/// Pool Noodle is a tool for ensuring that we maintain a bare minimum number of Firecracker Jails
/// for function execution. We wrap it in an Arc Mutex so we can update the queues it manages
/// across threads.
#[derive(Debug)]
pub struct PoolNoodle<I, S: Spec>(Arc<PoolNoodleInner<I, S>>);

impl<I, E, S> Clone for PoolNoodle<I, S>
where
    I: Instance<Error = E> + Send + Sync + 'static,
    S: Spec<Error = E, Instance = I> + Send + Sync + 'static,
    E: Send,
{
    fn clone(&self) -> Self {
        PoolNoodle(self.0.clone())
    }
}

impl<I, E, S> PoolNoodle<I, S>
where
    I: Instance<Error = E> + Send + Sync + 'static,
    S: Spec<Error = E, Instance = I> + Clone + Send + Sync + 'static,
    E: Send + Display + 'static,
{
    /// Creates a new instance of PoolNoodle
    pub fn new(config: PoolNoodleConfig<S>) -> Self {
        let pool_size = config.pool_size;
        let pool = PoolNoodle(Arc::new(PoolNoodleInner::new(config)));
        // start by cleaning jails just to make sure
        for id in 1..=pool_size {
            pool.inner().push_clean_task_to_work_queue(id);
        }
        pool
    }

    /// do the thing
    pub fn run(&mut self) -> Result<(), E> {
        if self.inner().check_health {
            if let Some(err) = futures::executor::block_on(self.check_health()).err() {
                return Err(err);
            }
        }
        let inner = self.inner();

        // for each worker, spin up a thread to pull work
        tokio::spawn(async move {
            for _ in 0..inner.max_concurrency {
                let inner = inner.clone();
                tokio::spawn(Self::spawn_worker(inner));
            }
        });
        Ok(())
    }

    fn inner(&self) -> Arc<PoolNoodleInner<I, S>> {
        Arc::clone(&self.0)
    }

    async fn spawn_worker(inner: Arc<PoolNoodleInner<I, S>>) {
        loop {
            let inner = inner.clone();
            tokio::select! {
                _ = inner.shutdown_token.cancelled() => {
                    debug!("main loop received cancellation");
                    break;
                }

                Some(task_type) = async { inner.work_queue.pop() } => {
                    inner.handle_task(task_type).await;
                }

                _ = time::sleep(Duration::from_millis(1)) => {}
            }
        }
    }

    /// This will attempt to get a ready, healthy instance from the pool.
    /// If there are no instances, it will give the main loop a chance to fill the pool and try
    /// again. It will throw an error if there are no available instances after enough retries.
    pub async fn get(&self) -> Result<LifeGuard<I, E, S>, E> {
        metric!(counter.pool_noodle.get_requests = 1);
        let inner = self.inner();

        let max_retries = self.inner().retry_limit; // Set the maximum number of retries
        let mut retries = 0;
        loop {
            if retries >= max_retries {
                return Err(PoolNoodleError::ExecutionPoolStarved);
            }
            if let Some(mut instance) = inner.ready_queue.pop() {
                metric!(counter.pool_noodle.ready = -1);
                // Try to ensure the item is healthy
                match &mut instance.ensure_healthy().await {
                    Ok(_) => {
                        metric!(counter.pool_noodle.get_requests = -1);
                        metric!(counter.pool_noodle.active = 1);
                        return Ok(LifeGuard::new(Some(instance), inner.clone()));
                    }
                    Err(_) => {
                        debug!("PoolNoodle: not healthy, cleaning up and getting a new one.");
                        drop(instance);
                    }
                }
            } else {
                retries += 1;
                debug!(
                    "Failed to get from pool, retry ({} of {})",
                    retries, max_retries
                );
                sleep(Duration::from_millis(10)).await;
            }
        }
    }

    async fn check_health(&mut self) -> Result<(), E> {
        info!("verifying instance lifecycle health");
        let id = 0;
        let mut task = PoolNoodleTask::new(None, id, self.inner().spec.clone());
        info!("cleaning...");
        task.clean().await?;
        info!("preparing...");
        task.prepare().await?;
        info!("spawning...");
        let mut i = task.spawn().await?;
        info!("checking...");
        i.ensure_healthy()
            .await
            .map_err(|err| PoolNoodleError::Unhealthy(err))?;
        info!("terminating...");
        task.set_instance(Some(i));
        task.terminate().await?;
        self.inner()
            .spec
            .clean(id)
            .await
            .map_err(|err| PoolNoodleError::InstanceClean(err))?;
        info!("instance lifecycle is good!");
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct PoolNoodleInner<I, S>
where
    S: Spec,
{
    check_health: bool,
    max_concurrency: u32,
    ready_queue: ArrayQueue<I>,
    retry_limit: u32,
    shutdown_token: CancellationToken,
    spec: S,
    work_queue: ArrayQueue<PoolNoodleTaskType<I, S>>,
}

impl<I, E, S> PoolNoodleInner<I, S>
where
    I: Instance<Error = E> + Send + Sync + 'static,
    S: Spec<Error = E, Instance = I> + Clone + Send + Sync + 'static,
    E: Send + Display + 'static,
{
    fn new(config: PoolNoodleConfig<S>) -> Self {
        info!(
            "creating a pool of size {} with concurrency of {} ",
            config.pool_size, config.max_concurrency
        );
        Self {
            check_health: config.check_health,
            max_concurrency: config.max_concurrency,
            ready_queue: ArrayQueue::new(config.pool_size as usize),
            retry_limit: config.retry_limit,
            shutdown_token: config.shutdown_token,
            spec: config.spec,
            work_queue: ArrayQueue::new(config.pool_size as usize),
        }
    }

    async fn handle_task(self: Arc<Self>, task_type: PoolNoodleTaskType<I, S>) {
        match task_type {
            PoolNoodleTaskType::Clean(task) => self.handle_clean(task).await,
            PoolNoodleTaskType::Drop(task) => self.handle_drop(task).await,
            PoolNoodleTaskType::Prepare(task) => self.handle_prepare(task).await,
        }
    }

    async fn handle_clean(&self, task: PoolNoodleTask<I, S>) {
        metric!(counter.pool_noodle.task.clean = -1);
        let id = task.id();
        match task.clean().await {
            Ok(_) => {
                self.push_prepare_task_to_work_queue(id);
            }
            Err(e) => {
                warn!("PoolNoodle: failed to clean instance: {}", id);
                warn!("{}", e);
                self.push_clean_task_to_work_queue(id);
            }
        }
    }

    async fn handle_drop(&self, task: PoolNoodleTask<I, S>) {
        metric!(counter.pool_noodle.task.drop = -1);
        let id = task.id();
        match task.terminate().await {
            Ok(_) => {
                self.push_clean_task_to_work_queue(id);
            }
            Err(e) => {
                warn!("PoolNoodle: failed to drop instance: {}", id);
                warn!("{}", e);
            }
        }
    }

    async fn handle_prepare(&self, task: PoolNoodleTask<I, S>) {
        metric!(counter.pool_noodle.task.prepare = -1);
        let id = task.id();
        match &task.prepare().await {
            Ok(_) => match task.spawn().await {
                Ok(instance) => {
                    self.push_to_ready_queue(instance);
                }
                Err(e) => {
                    warn!("PoolNoodle: failed to start instance: {}", id);
                    warn!("{}", e);
                    self.push_clean_task_to_work_queue(id);
                }
            },
            Err(e) => {
                warn!("PoolNoodle: failed to ready instance: {}", id);
                warn!("{}", e);
                self.push_clean_task_to_work_queue(id);
            }
        }
    }

    fn push_clean_task_to_work_queue(&self, id: u32) {
        let task = PoolNoodleTaskType::Clean(PoolNoodleTask::new(None, id, self.spec.clone()));
        if self.work_queue.push(task).is_err() {
            warn!("failed to push instance to clean: {}", id);
        };
        metric!(counter.pool_noodle.task.clean = 1);
    }

    /// used by the instance guard implementation to handle drops
    pub(crate) fn push_drop_task_to_work_queue(&self, instance: I) {
        let id = instance.id();
        let task =
            PoolNoodleTaskType::Drop(PoolNoodleTask::new(Some(instance), id, self.spec.clone()));
        if self.work_queue.push(task).is_err() {
            warn!("failed to push instance to drop: {}", id);
        };
        metric!(counter.pool_noodle.task.drop = 1);
    }

    fn push_prepare_task_to_work_queue(&self, id: u32) {
        let task = PoolNoodleTaskType::Prepare(PoolNoodleTask::new(None, id, self.spec.clone()));
        if self.work_queue.push(task).is_err() {
            warn!("failed to push instance to prepare: {}", id);
        };
        metric!(counter.pool_noodle.task.prepare = 1);
    }

    fn push_to_ready_queue(&self, instance: I) {
        let id = instance.id();
        if self.ready_queue.push(instance).is_err() {
            warn!("failed to push to ready queue: {}", id);
        }
        metric!(counter.pool_noodle.ready = 1);
    }
}

#[cfg(test)]
mod tests {

    use std::fmt::{self, Formatter};

    use crate::instance::SpecBuilder;
    use async_trait::async_trait;
    use derive_builder::Builder;
    use tokio::time::{sleep, Duration};

    use super::*;

    pub struct DummyInstance {}

    #[derive(Clone)]
    pub struct DummyInstanceSpec {}
    #[async_trait]
    impl Spec for DummyInstanceSpec {
        type Instance = DummyInstance;
        type Error = DummyInstanceError;

        async fn clean(&self, _id: u32) -> result::Result<(), Self::Error> {
            Ok(())
        }
        async fn prepare(&self, _id: u32) -> result::Result<(), Self::Error> {
            Ok(())
        }
        async fn setup(&mut self) -> result::Result<(), Self::Error> {
            Ok(())
        }

        async fn spawn(&self, _id: u32) -> result::Result<Self::Instance, Self::Error> {
            Ok(DummyInstance {})
        }
    }
    #[derive(Builder, Default, Clone)]
    pub struct DummyInstanceBuilder {}
    impl SpecBuilder for DummyInstanceBuilder {
        type Spec = DummyInstanceSpec;
        type Error = DummyInstanceError;

        fn build(&self) -> result::Result<Self::Spec, Self::Error> {
            Ok(DummyInstanceSpec {})
        }
    }
    #[derive(Debug)]
    pub struct DummyInstanceError {}

    impl Display for DummyInstanceError {
        fn fmt(&self, _f: &mut Formatter) -> fmt::Result {
            Ok(())
        }
    }
    #[async_trait]
    impl Instance for DummyInstance {
        type SpecBuilder = DummyInstanceBuilder;
        type Error = DummyInstanceError;

        async fn terminate(&mut self) -> result::Result<(), Self::Error> {
            Ok(())
        }

        async fn ensure_healthy(&mut self) -> result::Result<(), Self::Error> {
            Ok(())
        }

        fn id(&self) -> u32 {
            0
        }
    }
    #[tokio::test]
    async fn pool_noodle_lifecycle() {
        let shutdown_token = CancellationToken::new();

        let spec = DummyInstanceSpec {};

        let config = PoolNoodleConfig {
            check_health: false,
            max_concurrency: 10,
            pool_size: 3,
            retry_limit: 3,
            shutdown_token: shutdown_token.clone(),
            spec,
        };
        let mut pool = PoolNoodle::new(config);
        pool.run().expect("failed to start");

        // give the pool time to create some instances
        sleep(Duration::from_millis(500)).await;
        // go get an instance
        let mut instance = pool.get().await.expect("should be able to get an instance");
        instance.ensure_healthy().await.expect("failed healthy");
        drop(instance);

        let a = pool.get().await.expect("should be able to get an instance");
        let b = pool.get().await.expect("should be able to get an instance");
        let c = pool.get().await.expect("should be able to get an instance");
        drop(a);
        drop(b);
        drop(c);
        shutdown_token.cancel();
        assert!(pool.get().await.is_err());
    }
}
