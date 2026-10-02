use crate::{http::ApiError, input::Normalized};
use bytes::Bytes;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, atomic::Ordering},
};
use workbench_core::{
    RunControl,
    result::{OperationResult, Status, new_id, timestamp},
};

const MAX_HISTORY: usize = 50;
const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;
const MAX_SUBMISSIONS: usize = 1024;

#[derive(Clone, Serialize)]
pub(crate) struct Summary {
    pub id: String,
    operation: String,
    check_label: &'static str,
    root_id: Option<String>,
    state: &'static str,
    submitted_at: String,
    execution_status: Option<Status>,
    receipt_url: Option<String>,
}

struct Record {
    summary: Summary,
    identity: [u8; 32],
    receipt: Option<Bytes>,
}

pub(crate) struct Work {
    pub index: usize,
    pub request: workbench_core::request::Request,
    pub control: RunControl,
}

#[derive(Default)]
pub(crate) struct State {
    records: Vec<Record>,
    submissions: HashMap<String, usize>,
    retained: VecDeque<usize>,
    retained_bytes: usize,
    evicted_count: usize,
    active: Option<(usize, Arc<std::sync::atomic::AtomicUsize>)>,
    pub stopped: bool,
}

impl State {
    pub fn submit(&mut self, input: Normalized) -> Result<(Summary, Option<Work>), ApiError> {
        if let Some(&index) = self.submissions.get(&input.submission_id) {
            let record = &self.records[index];
            if record.identity != input.identity {
                return Err(ApiError::new(
                    409,
                    "submission-conflict",
                    "Submission ID already used",
                    "Reconcile the original submission; do not reuse its ID for different inputs.",
                ));
            }
            return Ok((record.summary.clone(), None));
        }
        if self.stopped {
            return Err(ApiError::new(
                503,
                "session-stopped",
                "Session stopped",
                "Restart the server; shutdown or uncertain worker cleanup closed admission.",
            ));
        }
        if self.records.len() == MAX_SUBMISSIONS {
            return Err(ApiError::new(
                503,
                "session-capacity",
                "Session submission limit reached",
                "Start a new server session to run more checks.",
            ));
        }
        if self.active.is_some() {
            return Err(ApiError::new(
                409,
                "run-in-progress",
                "A check is already running",
                "Wait for the current check or cancel it.",
            ));
        }
        let summary = Summary {
            id: new_id("inv"),
            operation: input.request.operation.clone(),
            check_label: input.label,
            root_id: input.root_id,
            state: "running",
            submitted_at: timestamp(),
            execution_status: None,
            receipt_url: None,
        };
        let index = self.records.len();
        let control = RunControl::default();
        self.active = Some((index, Arc::clone(&control.signal)));
        self.submissions.insert(input.submission_id, index);
        self.records.push(Record {
            summary: summary.clone(),
            identity: input.identity,
            receipt: None,
        });
        Ok((
            summary,
            Some(Work {
                index,
                request: input.request,
                control,
            }),
        ))
    }

    pub fn finish(&mut self, index: usize, receipt: OperationResult) {
        if receipt
            .errors
            .iter()
            .any(|error| error.code == "cleanup_unconfirmed")
        {
            self.stopped = true;
        }
        let record = &mut self.records[index];
        record.summary.state = "terminal";
        record.summary.execution_status = Some(receipt.execution.status.clone());
        record.summary.receipt_url = Some(format!("/api/v1/runs/{}/receipt", record.summary.id));
        let bytes = Bytes::from(receipt.json_bytes());
        self.retained_bytes += bytes.len();
        record.receipt = Some(bytes);
        self.retained.push_back(index);
        self.active = None;
        while self.retained.len() > MAX_HISTORY || self.retained_bytes > MAX_HISTORY_BYTES {
            self.evict_oldest();
        }
    }

    fn evict_oldest(&mut self) {
        if let Some(index) = self.retained.pop_front() {
            if let Some(bytes) = self.records[index].receipt.take() {
                self.retained_bytes -= bytes.len();
            }
            self.evicted_count += 1;
        }
    }

    fn index(&self, id: &str) -> Result<usize, ApiError> {
        self.records
            .iter()
            .position(|record| record.summary.id == id)
            .ok_or_else(ApiError::not_found)
    }

    pub fn summary(&self, id: &str) -> Result<Summary, ApiError> {
        Ok(self.records[self.index(id)?].summary.clone())
    }

    pub fn receipt(&self, id: &str) -> Result<Bytes, ApiError> {
        let record = &self.records[self.index(id)?];
        if record.summary.state != "terminal" {
            return Err(ApiError::new(
                409,
                "receipt-pending",
                "Receipt is not ready",
                "Poll the invocation until its worker returns a terminal receipt.",
            ));
        }
        record.receipt.clone().ok_or_else(|| {
            ApiError::new(
                410,
                "receipt-evicted",
                "Receipt no longer retained",
                "This session's bounded history was cleared or evicted this receipt.",
            )
        })
    }

    pub fn cancel(&mut self, id: &str) -> Result<(u16, Summary), ApiError> {
        let index = self.index(id)?;
        let record = &mut self.records[index];
        if let Some((active, signal)) = &self.active
            && *active == index
        {
            signal.store(2, Ordering::Relaxed);
            record.summary.state = "cancelling";
            return Ok((202, record.summary.clone()));
        }
        Ok((200, record.summary.clone()))
    }

    pub fn list(&self, limit: usize, offset: usize) -> Value {
        let mut indices = self.retained.iter().copied().collect::<Vec<_>>();
        if let Some((index, _)) = self.active {
            indices.push(index);
        }
        indices.reverse();
        let count = indices.len();
        let runs = indices
            .into_iter()
            .take(50)
            .skip(offset)
            .take(limit)
            .map(|i| &self.records[i].summary)
            .collect::<Vec<_>>();
        // There may be 50 terminals plus an active invocation. The API bounds offset at49;
        // limit the visible list to its newest50 while preserving the50 terminal receipts.
        let next = (offset + limit < count.min(50)).then_some(offset + limit);
        json!({"runs":runs,"next_offset":next,"evicted_count":self.evicted_count})
    }

    pub fn clear(&mut self) -> Value {
        let removed = self.retained.len();
        while !self.retained.is_empty() {
            self.evict_oldest();
        }
        json!({"removed":removed,"active_preserved":true})
    }

    pub fn shutdown(&mut self, signal: usize) {
        self.stopped = true;
        if let Some((index, control)) = &self.active {
            control.store(signal, Ordering::Relaxed);
            self.records[*index].summary.state = "cancelling";
        }
    }
}

pub(crate) type SharedState = Arc<Mutex<State>>;

#[cfg(test)]
mod tests {
    use super::*;
    fn input(id: usize) -> Normalized {
        Normalized {
            submission_id: format!("00000000-0000-0000-0000-{id:012}"),
            request: workbench_core::request::Request::new("task.run", json!({})),
            root_id: None,
            label: "Error budget",
            identity: [0; 32],
        }
    }
    #[test]
    fn admission_has_no_queue_and_replays_original_even_after_clear() {
        let mut state = State::default();
        let (first, work) = state
            .submit(input(1))
            .unwrap_or_else(|_| panic!("first admission"));
        let busy = state.submit(input(2)).err().expect("busy");
        assert_eq!(busy.status, 409);
        let (duplicate, none) = state
            .submit(input(1))
            .unwrap_or_else(|_| panic!("duplicate"));
        assert_eq!(first.id, duplicate.id);
        assert!(none.is_none());
        let work = work.unwrap();
        state.finish(work.index, OperationResult::new(&work.request));
        assert_eq!(state.clear()["removed"], 1);
        let (duplicate, none) = state
            .submit(input(1))
            .unwrap_or_else(|_| panic!("duplicate after clear"));
        assert_eq!(duplicate.id, first.id);
        assert!(none.is_none());
        assert_eq!(state.receipt(&first.id).err().unwrap().status, 410);
        let mut changed = input(1);
        changed.identity[0] = 1;
        assert_eq!(state.submit(changed).err().unwrap().status, 409);
    }
    #[test]
    fn cancellation_waits_for_real_worker_result_and_clear_preserves_active() {
        let mut state = State::default();
        let (summary, work) = state
            .submit(input(1))
            .unwrap_or_else(|_| panic!("admission"));
        let work = work.unwrap();
        assert_eq!(state.clear()["removed"], 0);
        let (status, cancelling) = state
            .cancel(&summary.id)
            .unwrap_or_else(|_| panic!("cancel"));
        assert_eq!(status, 202);
        assert_eq!(cancelling.state, "cancelling");
        assert_eq!(work.control.signal.load(Ordering::Relaxed), 2);
        assert_eq!(state.receipt(&summary.id).err().unwrap().status, 409);
        state.finish(work.index, OperationResult::new(&work.request));
        let (status, terminal) = state
            .cancel(&summary.id)
            .unwrap_or_else(|_| panic!("terminal cancel"));
        assert_eq!(status, 200);
        assert_eq!(terminal.execution_status, Some(Status::Succeeded));
    }
    #[test]
    fn history_count_bytes_and_submission_tombstones_are_bounded() {
        let mut state = State::default();
        let mut first = String::new();
        for id in 0..MAX_SUBMISSIONS {
            let (summary, work) = state
                .submit(input(id))
                .unwrap_or_else(|_| panic!("admission"));
            if id == 0 {
                first = summary.id;
            }
            let work = work.unwrap();
            let mut receipt = OperationResult::new(&work.request);
            if id < 12 {
                receipt.output.stdout = "x".repeat(1_500_000);
            }
            state.finish(work.index, receipt);
            assert!(state.retained.len() <= 50);
            assert!(state.retained_bytes <= MAX_HISTORY_BYTES);
        }
        assert_eq!(state.records.len(), MAX_SUBMISSIONS);
        assert_eq!(state.receipt(&first).err().unwrap().status, 410);
        assert_eq!(
            state.submit(input(MAX_SUBMISSIONS)).err().unwrap().status,
            503
        );
    }
    #[test]
    fn unconfirmed_cleanup_closes_admission() {
        let mut state = State::default();
        let (_, work) = state
            .submit(input(0))
            .unwrap_or_else(|_| panic!("admission"));
        let work = work.unwrap();
        let mut receipt = OperationResult::new(&work.request);
        receipt.error("cleanup_unconfirmed", "cleanup is uncertain");
        state.finish(work.index, receipt);
        assert_eq!(state.submit(input(1)).err().unwrap().status, 503);
    }
}
