use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use futures_util::{StreamExt, future::pending, stream};

use crate::execution::docker::{discovery::PrerequisiteCode, engine::ImagePullControl};

#[path = "tests/retry.rs"]
mod retry;
#[path = "tests/retry_fence.rs"]
mod retry_fence;
#[path = "tests/retry_policy.rs"]
mod retry_policy;

struct DropSignal(Arc<AtomicBool>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn cancellation_drops_the_active_pull_stream() {
    let (progress_tx, _progress_rx) = mpsc::sync_channel(4);
    let control = ImagePullControl::new(progress_tx);
    let (started_tx, started_rx) = mpsc::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let signal = DropSignal(dropped.clone());
    let stream = stream::unfold((signal, Some(started_tx)), |(signal, started)| async move {
        if let Some(started) = started {
            let _ = started.send(());
        }
        let next = pending::<
            Option<(
                Result<bollard::models::CreateImageInfo, bollard::errors::Error>,
                (DropSignal, Option<mpsc::Sender<()>>),
            )>,
        >()
        .await;
        drop(signal);
        next
    })
    .boxed();

    let pull_control = control.clone();
    let pull = tokio::spawn(async move { super::consume_pull_stream(stream, &pull_control).await });
    tokio::task::spawn_blocking(move || started_rx.recv())
        .await
        .unwrap()
        .unwrap();
    control.cancel();

    let error = pull.await.unwrap().unwrap_err();
    assert_eq!(error.code, PrerequisiteCode::ImageUnavailable);
    assert!(dropped.load(Ordering::Acquire));
}

fn progress(current: i64) -> Result<bollard::models::CreateImageInfo, bollard::errors::Error> {
    Ok(bollard::models::CreateImageInfo {
        id: Some("stable-layer".into()),
        progress_detail: Some(bollard::models::ProgressDetail {
            current: Some(current),
            total: Some(100),
        }),
        ..Default::default()
    })
}

fn error_detail(
    code: Option<i64>,
    message: &str,
) -> Result<bollard::models::CreateImageInfo, bollard::errors::Error> {
    Ok(bollard::models::CreateImageInfo {
        error_detail: Some(bollard::models::ErrorDetail {
            code,
            message: Some(message.to_owned()),
        }),
        ..Default::default()
    })
}
