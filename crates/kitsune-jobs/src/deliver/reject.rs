use crate::{JobRunnerContext, Runnable};
use diesel::{OptionalExtension, QueryDsl};
use diesel_async::RunQueryDsl;
use kitsune_core::traits::deliverer::Action;
use kitsune_db::{model::follower::Follow, schema::accounts_follows};
use scoped_futures::ScopedFutureExt;
use serde::{Deserialize, Serialize};
use speedy_uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct DeliverReject {
    pub follow_id: Uuid,
}

impl Runnable for DeliverReject {
    type Context = JobRunnerContext;
    type Error = miette::Report;

    #[instrument(skip_all, fields(follow_id = %self.follow_id))]
    async fn run(&self, ctx: &Self::Context) -> Result<(), Self::Error> {
        let follow = ctx
            .db_pool
            .with_connection(|db_conn| {
                async move {
                    accounts_follows::table
                        .find(self.follow_id)
                        .get_result::<Follow>(db_conn)
                        .await
                        .optional()
                }
                .scoped()
            })
            .await?;

        let Some(follow) = follow else {
            return Ok(());
        };

        ctx.deliverer
            .deliver(Action::RejectFollow(follow))
            .await
            .map_err(|err| miette::Report::new_boxed(err.into()))?;

        ctx.db_pool
            .with_connection(|db_conn| {
                diesel::delete(accounts_follows::table.find(self.follow_id))
                    .execute(db_conn)
                    .scoped()
            })
            .await?;

        Ok(())
    }
}
