use tauri::async_runtime::JoinHandle;

#[derive(Default)]
pub(super) struct SessionTasks {
    preparation: Option<JoinHandle<()>>,
    probe: Option<JoinHandle<()>>,
}

impl SessionTasks {
    pub(super) fn replace_preparation(&mut self, task: JoinHandle<()>) {
        abort_task(&mut self.preparation);
        self.preparation = Some(task);
    }

    pub(super) fn finish_preparation(&mut self) {
        self.preparation.take();
    }

    pub(super) fn replace_probe(&mut self, task: JoinHandle<()>) {
        abort_task(&mut self.probe);
        self.probe = Some(task);
    }

    pub(super) fn finish_probe(&mut self) {
        self.probe.take();
    }

    pub(super) fn abort_all(&mut self) {
        abort_task(&mut self.preparation);
        abort_task(&mut self.probe);
    }
}

impl Drop for SessionTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}

fn abort_task(slot: &mut Option<JoinHandle<()>>) {
    if let Some(task) = slot.take() {
        task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use std::time::Duration;
    use tauri::async_runtime::{self, JoinHandle};
    use tokio::sync::oneshot;

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    fn tracked_pending_task() -> (JoinHandle<()>, oneshot::Receiver<()>, oneshot::Receiver<()>) {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let task = async_runtime::spawn(async move {
            let _drop_signal = DropSignal(Some(dropped_tx));
            let _ = started_tx.send(());
            pending::<()>().await;
        });
        (task, started_rx, dropped_rx)
    }

    #[tokio::test]
    async fn abort_all_drops_preparation_and_probe_tasks() {
        let (preparation, preparation_started, preparation_dropped) = tracked_pending_task();
        let (probe, probe_started, probe_dropped) = tracked_pending_task();
        let mut tasks = SessionTasks::default();
        tasks.replace_preparation(preparation);
        tasks.replace_probe(probe);
        preparation_started.await.unwrap();
        probe_started.await.unwrap();

        tasks.abort_all();

        tokio::time::timeout(Duration::from_millis(100), preparation_dropped)
            .await
            .expect("preparation task was not aborted")
            .unwrap();
        tokio::time::timeout(Duration::from_millis(100), probe_dropped)
            .await
            .expect("probe task was not aborted")
            .unwrap();
    }

    #[tokio::test]
    async fn replacing_probe_aborts_previous_task() {
        let (first, first_started, first_dropped) = tracked_pending_task();
        let (second, second_started, mut second_dropped) = tracked_pending_task();
        let mut tasks = SessionTasks::default();
        tasks.replace_probe(first);
        first_started.await.unwrap();

        tasks.replace_probe(second);
        second_started.await.unwrap();

        tokio::time::timeout(Duration::from_millis(100), first_dropped)
            .await
            .expect("replaced probe task was not aborted")
            .unwrap();
        assert!(second_dropped.try_recv().is_err());
        tasks.abort_all();
    }
}
