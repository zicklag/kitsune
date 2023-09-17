#[macro_use]
extern crate tracing;

use athena::JobQueue;
use kitsune_core::{
    activitypub::Deliverer,
    config::JobQueueConfiguration,
    job::{JobRunnerContext, KitsuneContextRepo},
    state::State as CoreState,
};
use kitsune_db::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::task::JoinSet;

const EXECUTION_TIMEOUT_DURATION: Duration = Duration::from_secs(30);

pub fn prepare_job_queue(
    db_pool: PgPool,
    config: &JobQueueConfiguration,
) -> Result<JobQueue<KitsuneContextRepo>, deadpool_redis::CreatePoolError> {
    let context_repo = KitsuneContextRepo::builder().db_pool(db_pool).build();
    let redis_pool = deadpool_redis::Config::from_url(config.redis_url.as_str())
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))?;

    let queue = JobQueue::builder()
        .context_repository(context_repo)
        .queue_name("kitsune-jobs")
        .redis_pool(redis_pool)
        .build();

    Ok(queue)
}

#[instrument(skip(job_queue, state))]
pub async fn run_dispatcher(
    job_queue: JobQueue<KitsuneContextRepo>,
    state: CoreState,
    num_job_workers: usize,
) {
    let deliverer = Deliverer::builder()
        .federation_filter(state.service.federation_filter.clone())
        .build();
    let ctx = Arc::new(JobRunnerContext { deliverer, state });

    let mut job_joinset = JoinSet::new();
    loop {
        while let Err(error) = job_queue
            .spawn_jobs(
                num_job_workers - job_joinset.len(),
                Arc::clone(&ctx),
                &mut job_joinset,
            )
            .await
        {
            error!(?error, "failed to spawn more jobs");
            just_retry::sleep_a_bit().await;
        }

        let join_all = async { while job_joinset.join_next().await.is_some() {} };
        let _ = tokio::time::timeout(EXECUTION_TIMEOUT_DURATION, join_all).await;
    }
}