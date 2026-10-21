# Dependency and personal-use record

The application is for the owner's personal, non-commercial use. No commercial
business or distribution requirement has been introduced. This repository does not
assign a public licence to the owner's source or publish it to a remote.

`Cargo.lock` pins the exact dependencies and checksums. The adjacent inventory
records all 83 resolved registry packages, their declared licences, upstream
repositories and available local licence-file SHA-256 values. This includes inactive
platform and development packages, not 83 shipped runtime components. Regenerate it
with `python3 tools/dependency_inventory.py` after fetching locked dependencies.

| Material | Personal/local implementation | Publishing the owner's original source | Redistributing third-party material |
| --- | --- | --- | --- |
| Core Rust crates | Reviewed permissive MIT route where offered; subtle uses BSD-3-Clause, unicode-ident additionally Unicode-3.0 | Cargo references and original integration code do not copy proprietary SDK/model code into the repository | Include applicable copyright, permission and disclaimer notices if distributing their source/binaries; prepare target-specific notices before release |
| Bundled SQLite 3.46.0 through libsqlite3-sys | SQLite source copyright disclaimer inspected; Rust wrapper MIT | No SQLite source is committed | Recheck bundled component/target and notices when distributing a binary; current lock is not a vulnerability audit |
| r-efi 6.0.0 (inactive UEFI target dependency) | Metadata/README declares MIT OR Apache-2.0 OR LGPL-2.1-or-later; MIT route selected | No r-efi source copied | Fetched crate lacks top-level licence text; retain notice-verification item before any relevant redistribution |
| Recogniser SDKs, model weights and dictionaries | None integrated or approved by this implementation | None copied | Personal-use rights, source publication permissions and resource redistribution rights must each be evidenced for the actual selected model/version |
| Synthetic test adapters/strokes | Authored for this repository; fixtures contain no owner's handwriting | Part of original test code | Must never be presented as a real recogniser or measured dataset |

The licence choices are dependency engineering records, not claims that source and
weight licences are interchangeable. No proprietary SDK, credential, vendor licence
key, model weight or private notebook is committed. A model descriptor requires a
personal-use evidence reference but that string alone does not constitute a licence
review. The durable installation ledger and actual model-specific review are pending.

The database is not SQLCipher; the SQLCipher source shipped inside a dependency's
crate is not enabled. Content locks must not be described as database encryption.
Cryptographic dependencies and this envelope still require release security review;
no dependency vulnerability scan or independent cryptographic audit is claimed.
