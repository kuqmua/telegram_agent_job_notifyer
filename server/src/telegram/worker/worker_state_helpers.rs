use std::sync::mpsc::Receiver;

use crate::runtime::ServiceState;

pub(super) fn drain_progress_receiver_into_buffer(
    process_progress_receiver: &Receiver<String>,
    process_output_buffer: &mut String,
) {
    while let Ok(progress_text_chunk) = process_progress_receiver.try_recv() {
        process_output_buffer.push_str(&progress_text_chunk);
    }
}

pub(super) async fn refresh_task_queue_depth_metric(runtime_state: &ServiceState) {
    let task_queue_depth = runtime_state.task_manager().task_queue_depth().await;
    runtime_state
        .metrics()
        .set_task_queue_depth(task_queue_depth);
}
