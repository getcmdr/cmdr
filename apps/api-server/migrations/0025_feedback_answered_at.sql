-- When this feedback message stopped owing its sender a reply. Replies go out from David's own mailbox, which the
-- Worker never sees, so the cron copies it from the message's board card (`src/report-replies.ts`): NULL while the
-- card is open with `needs-reply` (or the row has no card), stamped once it closes or loses the label.
ALTER TABLE feedback ADD COLUMN answered_at TEXT;
