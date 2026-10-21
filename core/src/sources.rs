//! Source links do not fetch data or grant access. Only the trusted platform host
//! supplies bytes after a deliberate document-provider grant. Authorised snapshots
//! are distinct from disposable live-source caches (owner Q383).
use crate::{Error,Id,Result,digest,now_ms,security::Access,store::Store};
use rusqlite::{Connection,OptionalExtension};
use serde::{Serialize,Deserialize};

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
pub enum Availability { Available,Missing,Revoked }
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
pub enum Acquisition { ExternalLink,ManagedCopy }
#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfView {pub page:u32,pub zoom:f64,pub pan_x:f64,pub pan_y:f64,pub divider:f64}
impl Default for PdfView{fn default()->Self{Self{page:0,zoom:1.,pan_x:0.,pan_y:0.,divider:0.5}}}
impl PdfView{fn validate(&self)->Result<()>{if !self.zoom.is_finite()||self.zoom<=0.||!self.pan_x.is_finite()||!self.pan_y.is_finite()||!self.divider.is_finite()||self.divider<=0.||self.divider>=1.{return Err(Error::Invalid("PDF view"));}Ok(())}}

#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id:Id,pub context:Id,pub locator:String,pub revision:String,
    pub availability:Availability,pub acquisition:Acquisition,
    pub previous_revisions:Vec<String>,pub view:PdfView,
}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)]
pub enum AssetClass { LiveCache,ManagedCopy,HistoricalSnapshot }
#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retention {
    pub source_revision:String,pub captured_ms:i64,
    /// Independent continuing authorisation, not an inference from past access.
    pub authority:String,pub export_allowed:bool,
}
#[derive(Debug,Clone,PartialEq)]
pub struct SourceBytes {pub bytes:Vec<u8>,pub historical:bool,pub availability:Availability,pub source_revision:String}

impl Store {
    pub(crate) fn raw_source(db:&Connection,id:Id)->Result<Source>{
        let json:String=db.query_row("SELECT payload FROM sources WHERE id=?1",[id.to_string()],|r|r.get(0)).optional()?.ok_or(Error::Unavailable)?;
        Ok(serde_json::from_str(&json)?)
    }
    pub(crate) fn read_source(db:&Connection,id:Id,access:&Access)->Result<Source>{let s=Self::raw_source(db,id)?;Self::authorise(db,s.context,access,false)?;Ok(s)}
    fn save_source(db:&Connection,s:&Source)->Result<()>{db.execute("UPDATE sources SET payload=?1 WHERE id=?2",rusqlite::params![serde_json::to_string(s)?,s.id.to_string()])?;Ok(())}
    pub fn source(&self,id:Id,access:&Access)->Result<Source>{Self::read_source(&self.db,id,access)}
    pub fn link_pdf(&mut self,context:Id,locator:&str,revision:&str,access:&Access)->Result<Source>{
        if locator.is_empty()||locator.len()>4096||(!locator.starts_with("content://")&&!locator.starts_with("file://")) {return Err(Error::Invalid("local PDF locator"));}
        if revision.len()!=64||!revision.bytes().all(|c|c.is_ascii_hexdigit()){return Err(Error::Invalid("source digest"));}
        let s=Source{id:Id::new(),context,locator:locator.into(),revision:revision.to_ascii_lowercase(),availability:Availability::Available,acquisition:Acquisition::ExternalLink,previous_revisions:vec![],view:PdfView::default()};
        self.write(|db|{Self::authorise(db,context,access,false)?;db.execute("INSERT INTO sources VALUES(?1,?2,?3)",rusqlite::params![s.id.to_string(),context.to_string(),serde_json::to_string(&s)?])?;Ok(s.clone())})
    }
    pub fn set_pdf_view(&mut self,id:Id,view:PdfView,access:&Access)->Result<()>{view.validate()?;self.write(|db|{let mut s=Self::read_source(db,id,access)?;s.view=view;Self::save_source(db,&s)})}
    pub fn mark_source_missing(&mut self,id:Id,access:&Access)->Result<()>{self.write(|db|{let mut s=Self::read_source(db,id,access)?;if s.availability==Availability::Revoked{return Err(Error::Denied);}s.availability=Availability::Missing;Self::save_source(db,&s)})}
    pub fn relink_pdf(&mut self,id:Id,locator:&str,revision:&str,confirmed:bool,access:&Access)->Result<()>{
        if !confirmed{return Err(Error::ReviewRequired);}
        if !locator.starts_with("content://")&&!locator.starts_with("file://"){return Err(Error::Invalid("local PDF locator"));}
        self.write(|db|{
            let mut s=Self::read_source(db,id,access)?;
            if s.availability==Availability::Revoked{return Err(Error::Denied);}
            if revision!=s.revision{return Err(Error::ReviewRequired);}
            s.locator=locator.into();s.availability=Availability::Available;Self::save_source(db,&s)
        })
    }
    pub fn retain_source_bytes(&mut self,id:Id,bytes:&[u8],class:AssetClass,authority:Option<&str>,export_allowed:bool,consented:bool,access:&Access)->Result<Id>{
        if !consented{return Err(Error::Denied);}if bytes.len()>64*1024*1024{return Err(Error::ResourceLimit);}
        if class==AssetClass::HistoricalSnapshot&&authority.is_none_or(str::is_empty){return Err(Error::Denied);}
        self.write(|db|{
            let mut s=Self::read_source(db,id,access)?;
            if s.availability!=Availability::Available{return Err(Error::Denied);}
            if class==AssetClass::ManagedCopy&&digest(bytes)!=s.revision{return Err(Error::Integrity);}
            if class==AssetClass::ManagedCopy&&!bytes.starts_with(b"%PDF-"){return Err(Error::Invalid("PDF signature"));}
            let retention=authority.filter(|v|!v.is_empty()).map(|a|Retention{source_revision:s.revision.clone(),captured_ms:now_ms(),authority:a.into(),export_allowed});
            if class==AssetClass::LiveCache&&retention.is_some(){return Err(Error::Invalid("cache retention exception"));}
            let asset=Id::new();db.execute("INSERT INTO source_assets VALUES(?1,?2,?3,?4,?5,?6)",rusqlite::params![asset.to_string(),id.to_string(),serde_json::to_string(&class)?,retention.as_ref().map(serde_json::to_string).transpose()?,bytes,digest(bytes)])?;
            if class==AssetClass::ManagedCopy{s.acquisition=Acquisition::ManagedCopy;Self::save_source(db,&s)?;}
            Ok(asset)
        })
    }
    pub fn revoke_source(&mut self,id:Id,access:&Access)->Result<()>{
        self.write(|db|{
            let mut s=Self::read_source(db,id,access)?;s.availability=Availability::Revoked;Self::save_source(db,&s)?;
            db.execute("DELETE FROM source_assets WHERE source=?1 AND authority IS NULL",[id.to_string()])?;Ok(())
        })
    }
    pub fn revoke_retention(&mut self,asset:Id,access:&Access)->Result<()>{
        self.write(|db|{let source:String=db.query_row("SELECT source FROM source_assets WHERE id=?1",[asset.to_string()],|r|r.get(0)).optional()?.ok_or(Error::Unavailable)?;Self::read_source(db,Id::parse(&source)?,access)?;db.execute("DELETE FROM source_assets WHERE id=?1",[asset.to_string()])?;Ok(())})
    }
    pub fn source_bytes(&self,asset:Id,for_export:bool,access:&Access)->Result<SourceBytes>{
        let (source,authority,bytes,hash):(String,Option<String>,Vec<u8>,String)=self.db.query_row("SELECT source,authority,bytes,digest FROM source_assets WHERE id=?1",[asset.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(Error::Unavailable)?;
        let s=Self::read_source(&self.db,Id::parse(&source)?,access)?;
        let retention:Option<Retention>=authority.as_deref().map(serde_json::from_str).transpose()?;
        if s.availability==Availability::Revoked&&retention.is_none(){return Err(Error::Denied);}
        if for_export&&retention.as_ref().is_none_or(|r|!r.export_allowed){return Err(Error::Denied);}
        if digest(&bytes)!=hash{return Err(Error::Integrity);}
        Ok(SourceBytes{bytes,historical:retention.is_some(),availability:s.availability,source_revision:retention.map(|r|r.source_revision).unwrap_or(s.revision)})
    }
}
