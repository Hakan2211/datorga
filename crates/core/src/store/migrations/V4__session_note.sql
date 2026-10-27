-- V4: Sitzungsnotiz. A note is a `report` of kind 'sitzungsnotiz' bound to its session; one per session.

CREATE UNIQUE INDEX idx_report_session_note ON report(session_id) WHERE kind = 'sitzungsnotiz';
