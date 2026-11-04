use crate::Result;
use std::future::Future;
use tokio::task::JoinSet;

/// Finish the whole contiguous read region before allowing a write or control to start.
/// Permission failures in an individual tool can produce a typed receipt; a global
/// dispatcher failure stops the next region after all already-started tasks have ended.
pub async fn execute<T, F, Fut>(items: &[T], run: F) -> Result<()>
where
    T: Clone + Send + 'static,
    F: Fn(T) -> Fut,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let mut tasks = JoinSet::new();
    let mut first = None;
    for item in items {
        if tasks.len() >= 8
            && let Some(done) = tasks.join_next().await
        {
            let outcome = done?;
            if let Err(err) = outcome {
                first.get_or_insert(err);
            }
        }
        tasks.spawn(run(item.clone()));
    }
    while let Some(done) = tasks.join_next().await {
        if let Err(err) = done? {
            first.get_or_insert(err);
        }
    }
    first.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn two_safe_reads_must_enter_before_either_completes() {
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            execute(&[1, 2], |_| {
                let b = barrier.clone();
                async move {
                    b.wait().await;
                    Ok(())
                }
            }),
        )
        .await
        .expect("reads did not overlap")
        .unwrap();
    }
}
