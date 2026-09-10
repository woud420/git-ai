use super::*;

pub(super) fn write_records(fixture: &Fixture, records: &[JjOperationEvidence]) {
    for record in records {
        crate::operations::jj::evidence::verify_evidence(JJ_OBSERVATION_READER_PROFILE, record)
            .unwrap();
        fs::write(
            operation_path(fixture, &record.operation_id),
            &record.operation_bytes,
        )
        .unwrap();
        fs::write(view_path(fixture, &record.view_id), &record.view_bytes).unwrap();
    }
}

pub(super) fn operation_path(fixture: &Fixture, id: &str) -> std::path::PathBuf {
    fixture.repo.join("op_store/operations").join(id)
}

pub(super) fn view_path(fixture: &Fixture, id: &str) -> std::path::PathBuf {
    fixture.repo.join("op_store/views").join(id)
}

pub(super) fn baseline(fixture: &Fixture, source: &str) -> DurableCurrentStateBaseline {
    let path = fixture.ancestor.join(format!("history-{source}.sqlite"));
    let anchors = [fixtures::merge()];
    let heads = vec![anchors[0].operation_id.clone()];
    let prepared =
        prepare_current_state_baseline(JJ_OBSERVATION_READER_PROFILE, &heads, &anchors).unwrap();
    let mut journal = JjObservationJournal::open_at_path(&path).unwrap();
    persist_current_state_baseline(&mut journal, source, 0, &prepared).unwrap();
    drop(journal);
    let journal = JjObservationJournal::open_at_path(&path).unwrap();
    reopen_current_state_baseline(&journal, source, &mut ReadBudget::new(9 * 1024 * 1024))
        .unwrap()
        .unwrap()
}

pub(super) fn fixture(records: &[JjOperationEvidence]) -> (Fixture, DurableCurrentStateBaseline) {
    let fixture = Fixture::new();
    create_seal(&fixture);
    let baseline = baseline(&fixture, SOURCE);
    write_records(&fixture, records);
    fixture.set_heads(&[&records.last().unwrap().operation_id]);
    (fixture, baseline)
}

pub(super) fn set_checkout(fixture: &Fixture, record: &JjOperationEvidence) {
    let mut bytes = vec![0x12, 64];
    for pair in record.operation_id.as_bytes().chunks_exact(2) {
        bytes.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
    }
    bytes.extend([0x1a, 7]);
    bytes.extend(b"default");
    fs::write(fixture.root.join(".jj/working_copy/checkout"), bytes).unwrap();
}

pub(super) fn collect(
    fixture: &Fixture,
    baseline: &DurableCurrentStateBaseline,
) -> CapturedJjHistoryEvidence {
    let mut budget = HistoryCaptureBudget::new(deadline());
    let mut current = budget.open(&fixture.context()).unwrap();
    canonical_policy(&mut current);
    current.collect_history(baseline).unwrap();
    current.final_recheck().unwrap();
    let history = current.into_history().unwrap();
    assert_eq!(budget.budget.counters().live_directory_descriptors, 0);
    history
}

pub(super) fn ids(history: &CapturedJjHistoryEvidence) -> Vec<&str> {
    history
        .ordered_operations()
        .iter()
        .map(|record| record.operation_id.as_str())
        .collect()
}

pub(super) fn remaining(budget: &HistoryCaptureBudget) -> (usize, usize) {
    (
        budget.budget.metadata.remaining_bytes(),
        budget.budget.metadata.remaining_file_attempts(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum HistoryEvent {
    Phase(HistoryPhase),
    Before(String),
    Verified(String),
}

pub(super) struct Trace<F> {
    pub inner: Hooks<fn(Event)>,
    pub events: Vec<HistoryEvent>,
    callback: F,
    pub expire_at: Option<HistoryPhase>,
}

impl<F: FnMut(&HistoryEvent)> Trace<F> {
    pub fn new(callback: F) -> Self {
        Self {
            inner: Hooks::new(|_| {}),
            events: Vec::new(),
            callback,
            expire_at: None,
        }
    }
    fn event(&mut self, event: HistoryEvent) {
        (self.callback)(&event);
        if matches!(&event, HistoryEvent::Phase(at) if Some(*at) == self.expire_at) {
            self.inner.now += Duration::from_secs(120);
        }
        self.events.push(event);
    }
}
impl<F: FnMut(&HistoryEvent)> CaptureHooks for Trace<F> {
    fn phase(&mut self, phase: CapturePhase) {
        CaptureHooks::phase(&mut self.inner, phase);
    }
    fn now(&mut self) -> Instant {
        self.inner.now
    }
}
impl<F: FnMut(&HistoryEvent)> SealHooks for Trace<F> {
    fn seal_sample(&mut self, phase: SealSample) {
        SealHooks::seal_sample(&mut self.inner, phase);
    }
}
impl<F: FnMut(&HistoryEvent)> HistoryHooks for Trace<F> {
    fn history_phase(&mut self, phase: HistoryPhase) {
        self.event(HistoryEvent::Phase(phase));
    }
    fn before_history_pair(&mut self, id: &str) {
        self.event(HistoryEvent::Before(id.to_owned()));
    }
    fn history_pair_verified(&mut self, id: &str) {
        self.event(HistoryEvent::Verified(id.to_owned()));
    }
}

pub(super) fn trace() -> Trace<impl FnMut(&HistoryEvent)> {
    Trace::new(|_| {})
}
