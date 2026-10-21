//! Bundled, offline help for implemented recovery behaviour. No network lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Trash,
    History,
    Retention,
    SaveFailure,
    MissingPdf,
    Recognition,
    BackupCredentials,
}
pub struct Help {
    pub title: &'static str,
    pub explanation: &'static str,
    pub next_action: &'static str,
}
pub fn topic(topic: Topic) -> Help {
    match topic{
        Topic::Trash=>Help{title:"Restore deleted work",explanation:"Items in Trash keep their original identity. Restoring an item reconnects its existing links. A permanently deleted item cannot be restored from Trash.",next_action:"Choose the item in Trash and restore it. If its original location is unavailable, restore that location first."},
        Topic::History=>Help{title:"Restore an earlier version",explanation:"Restoring a saved version creates a new version. Later work is not silently deleted. Content protection still applies.",next_action:"Review the earlier version, then choose Restore. Keep a separate backup before major changes."},
        Topic::Retention=>Help{title:"How long undo history lasts",explanation:"The default keeps 72 hours of active notebook use, adjustable up to 168 hours. Time away does not use this allowance. Older detailed edits may expire; current content and original handwriting remain.",next_action:"Choose a positive retention duration. Make an independent encrypted backup for longer-term recovery."},
        Topic::SaveFailure=>Help{title:"Changes need attention",explanation:"The latest operation was not confirmed saved. Previously saved content remains available. Saved locally and backed up are different states.",next_action:"Stop adding work, check free space, then retry. Preserve any unsaved work locally before closing."},
        Topic::MissingPdf=>Help{title:"Locate a reference PDF",explanation:"Your notes remain available if the linked PDF is missing. A file with the same name may be a different edition. Revoked sources cannot be opened from ordinary caches.",next_action:"Locate the original PDF and confirm the link. A different revision needs a separate reviewed replacement."},
        Topic::Recognition=>Help{title:"Keep writing when conversion fails",explanation:"Missing models, cancellation and failed conversion leave original handwriting intact. A proposal never changes your note until you accept it.",next_action:"Keep the ink, retry with a compatible local recogniser, or enter a manual correction. Review any result before accepting."},
        Topic::BackupCredentials=>Help{title:"Open an encrypted portable file",explanation:"Use its export passphrase or independently stored recovery key. Device biometrics alone cannot open this portable file. Losing both credentials can make it unrecoverable.",next_action:"Keep the recovery key separately and privately. After import, set protection on the receiving device."},
    }
}
