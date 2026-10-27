//! Bounded native encoding separated from live-library authorization.
use super::*;
use std::io::{self, Write};
pub(crate) const EXPORT_BYTES: usize = 64 * 1024 * 1024;
struct Budget {
    bytes: Option<Vec<u8>>,
    used: usize,
    exceeded: bool,
}
impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > EXPORT_BYTES.saturating_sub(self.used) {
            self.exceeded = true;
            return Err(io::Error::other("native export budget"));
        }
        self.used += bytes.len();
        if let Some(output) = &mut self.bytes {
            output.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn serialize(value: &impl Serialize, retain: bool) -> Result<Budget> {
    let mut output = Budget {
        bytes: retain.then(Vec::new),
        used: 0,
        exceeded: false,
    };
    match serde_json::to_writer(&mut output, value) {
        Ok(()) => Ok(output),
        Err(_) if output.exceeded => Err(Error::ResourceLimit),
        Err(error) => Err(error.into()),
    }
}
pub(super) fn encode_json(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(serialize(value, true)?.bytes.unwrap())
}
#[derive(Clone, Serialize)]
pub(crate) struct ExportStamp {
    library: String,
    generation: String,
    scopes: BTreeSet<Scope>,
}
impl ExportStamp {
    pub(crate) fn guard(&self) -> (&str, &BTreeSet<Scope>) {
        (&self.generation, &self.scopes)
    }
    pub(crate) fn new(library: String, generation: String, scopes: BTreeSet<Scope>) -> Self {
        Self {
            library,
            generation,
            scopes,
        }
    }
    pub(crate) fn verify(&self, auth: &mut dyn Authenticator) -> Result<()> {
        if !self.scopes.is_empty() && !auth.verify(Id::new(), &self.scopes) {
            return Err(Error::Denied);
        }
        Ok(())
    }
}
impl ExportPlan {
    pub(crate) fn stamp(&self) -> ExportStamp {
        ExportStamp {
            library: self.library.clone(),
            generation: self.generation.clone(),
            scopes: self.scopes.clone(),
        }
    }
    pub(crate) fn admission_bytes(&self) -> Result<usize> {
        Ok(serialize(&self.document, false)?.used)
    }
    pub(crate) fn encode(
        self,
        protection: ExportProtection<'_>,
        auth: &mut dyn Authenticator,
        acknowledge_omissions: bool,
        control: Option<&crate::processing::JobControl>,
    ) -> Result<Exported> {
        let check = || control.map_or(Ok(()), |control| control.check());
        check()?;
        if !self.omissions.is_empty() && !acknowledge_omissions {
            return Err(Error::ReviewRequired);
        }
        if !self.scopes.is_empty() && !auth.verify(Id::new(), &self.scopes) {
            return Err(Error::Denied);
        }
        check()?;
        if matches!(
            protection,
            ExportProtection::Plaintext {
                acknowledge_protected: false
            }
        ) && !self.scopes.is_empty()
        {
            return Err(Error::ReviewRequired);
        }
        let plain = zeroize::Zeroizing::new(self.document.encode()?);
        check()?;
        let output = match protection {
            ExportProtection::Encrypted { password } => {
                let encrypted = crypto::encrypt(&plain, password)?;
                Exported {
                    bytes: encrypted.bytes,
                    recovery_key: Some(encrypted.recovery_key),
                }
            }
            ExportProtection::Plaintext { .. } => Exported {
                bytes: plain.to_vec(),
                recovery_key: None,
            },
        };
        check()?;
        Ok(output)
    }
}
impl Store {
    pub(crate) fn validate_export_stamp(
        &self,
        stamp: &ExportStamp,
        access: Option<&Access>,
    ) -> Result<()> {
        if Self::setting(&self.db, "library_id")? != stamp.library
            || Self::setting(&self.db, "generation")? != stamp.generation
        {
            return Err(Error::Conflict);
        }
        if let Some(access) = access {
            Self::require_app(&self.db, access)?;
            if stamp.scopes.iter().any(|scope| !access.permits(scope)) {
                return Err(Error::Denied);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use serde::ser::SerializeSeq;
    struct Repeated;
    impl Serialize for Repeated {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            let text = "x".repeat(1024 * 1024);
            let mut seq = serializer.serialize_seq(Some(65))?;
            for _ in 0..65 {
                seq.serialize_element(&text)?;
            }
            seq.end()
        }
    }
    #[test]
    fn native_json_budget_bounds_counting_and_retained_encoding_without_partial_output() {
        assert!(matches!(
            serialize(&Repeated, false),
            Err(Error::ResourceLimit)
        ));
        assert!(matches!(encode_json(&Repeated), Err(Error::ResourceLimit)));
        assert_eq!(encode_json(&"a\"b").unwrap(), br#""a\"b""#);
    }
}
