mod common;
use common::*;
use stem_core::{Error, security::{Access,Scope}, store::{Kind,Store,LinkState,Order,FailurePoint,SaveState}};

#[test]
fn complete_parent_matrix_and_no_phantom_containers(){
    for parent in [Kind::Module,Kind::Project,Kind::Notebook,Kind::Note,Kind::Canvas]{
        for child in [Kind::Module,Kind::Project,Kind::Notebook,Kind::Note,Kind::Canvas]{
            let mut s=Store::memory().unwrap();let a=Access::public();
            let p=s.create(parent,None,"parent",&a).unwrap();
            let result=s.create(child,Some(p.id),"child",&a);
            assert_eq!(result.is_ok(),parent.accepts(child),"{parent:?} -> {child:?}");
            assert_eq!(s.list(Some(p.id),Order::Name,None,None,&a).unwrap().len(),usize::from(result.is_ok()));
        }
    }
}
#[test]
fn module_moves_check_descendants_cycles_and_depth_atomically(){
    let mut s=Store::memory().unwrap();let a=Access::public();
    let m=s.create(Kind::Module,None,"one",&a).unwrap();let child=s.create(Kind::Module,Some(m.id),"two",&a).unwrap();
    assert!(s.create(Kind::Module,Some(child.id),"three",&a).is_err());
    assert!(s.move_entity(m.id,0,Some(child.id),&a).is_err());
    let other=s.create(Kind::Module,None,"other",&a).unwrap();
    assert!(s.move_entity(m.id,0,Some(other.id),&a).is_err());
    assert_eq!(s.entity(m.id,&a).unwrap(),m);
    s.check_integrity().unwrap();
}
#[test]
fn unfiled_move_and_reopen_preserve_identity_creation_and_backlinks(){
    let dir=tempfile::tempdir().unwrap();let path=dir.path().join("notes.db");let a=Access::public();
    let mut s=Store::open(&path).unwrap();
    let note=s.create(Kind::Note,None,"same name",&a).unwrap();let other=s.create(Kind::Note,None,"same name",&a).unwrap();
    assert_ne!(note.id,other.id);s.add_link(other.id,note.id,None,&a).unwrap();
    let p=s.create(Kind::Project,None,"project",&a).unwrap();
    s.move_entity(note.id,0,Some(p.id),&a).unwrap();drop(s);
    let s=Store::open(path).unwrap();let moved=s.entity(note.id,&a).unwrap();
    assert_eq!(moved.created_ms,note.created_ms);assert_eq!(moved.parent,Some(p.id));
    assert_eq!(s.unfiled(&a).unwrap().len(),1);assert_eq!(s.links(note.id,true,&a).unwrap().len(),1);
}
#[test]
fn stale_edit_and_interrupted_move_do_not_change_saved_state(){
    let mut s=Store::memory().unwrap();let a=Access::public();let n=s.create(Kind::Note,None,"old",&a).unwrap();
    let renamed=s.rename(n.id,0,"new",&a).unwrap();assert!(matches!(s.rename(n.id,0,"lost update",&a),Err(Error::Conflict)));
    let p=s.create(Kind::Project,None,"p",&a).unwrap();s.inject_failure(FailurePoint::BeforeCommit);
    assert!(s.move_entity(n.id,renamed.revision,Some(p.id),&a).is_err());assert_eq!(s.entity(n.id,&a).unwrap(),renamed);
    assert_eq!(s.save_state(),SaveState::NeedsAttention);
}
#[test]
fn trash_dependency_review_restore_and_permanent_broken_link(){
    let mut s=Store::memory().unwrap();let a=Access::public();let n=s.create(Kind::Note,None,"n",&a).unwrap();let o=s.create(Kind::Note,None,"o",&a).unwrap();
    let l=s.add_link(o.id,n.id,None,&a).unwrap();let r=s.review_deletion(n.id,&a).unwrap();assert!(r.has_dependencies);
    assert!(matches!(s.trash(r,false,&a),Err(Error::ReviewRequired)));
    let r=s.review_deletion(n.id,&a).unwrap();s.trash(r,true,&a).unwrap();assert_eq!(s.follow_link(l.id,&a).unwrap(),LinkState::Unavailable);
    s.restore(n.id,&a).unwrap();assert!(matches!(s.follow_link(l.id,&a).unwrap(),LinkState::Available(_)));
    let r=s.review_deletion(n.id,&a).unwrap();s.trash(r,true,&a).unwrap();s.purge(n.id,&a).unwrap();assert!(s.restore(n.id,&a).is_err());assert_eq!(s.follow_link(l.id,&a).unwrap(),LinkState::Unavailable);
    s.remove_link(l.id,&a).unwrap();assert!(s.entity(o.id,&a).is_ok());
}
#[test]
fn delete_review_is_invalidated_by_new_dependency(){
    let mut s=Store::memory().unwrap();let a=Access::public();let n=s.create(Kind::Note,None,"n",&a).unwrap();let o=s.create(Kind::Note,None,"o",&a).unwrap();let r=s.review_deletion(n.id,&a).unwrap();s.add_link(o.id,n.id,None,&a).unwrap();assert!(matches!(s.trash(r,true,&a),Err(Error::Conflict)));
}
#[test]
fn locked_dependencies_disclose_presence_without_name_or_id(){
    let mut s=Store::memory().unwrap();let a=Access::public();let n=s.create(Kind::Note,None,"n",&a).unwrap();let secret=s.create(Kind::Note,None,"private",&a).unwrap();s.add_link(secret.id,n.id,None,&a).unwrap();s.set_lock(secret.id,0,true,&access([Scope::Entity(secret.id)])).unwrap();let review=s.review_deletion(n.id,&a).unwrap();assert!(review.has_dependencies);assert!(review.visible_dependants.is_empty());assert!(s.links(n.id,true,&a).unwrap().is_empty());
}
#[test]
fn app_lock_and_inherited_protection_do_not_grant_via_link_or_move(){
    let mut s=Store::memory().unwrap();let a=Access::public();let p=s.create(Kind::Project,None,"p",&a).unwrap();let n=s.create(Kind::Note,Some(p.id),"n",&a).unwrap();let o=s.create(Kind::Note,None,"o",&a).unwrap();let l=s.add_link(o.id,n.id,None,&a).unwrap();
    let auth=access([Scope::Entity(p.id),Scope::Entity(n.id),Scope::App]);s.set_lock(p.id,0,true,&auth).unwrap();assert!(s.entity(n.id,&a).is_err());assert_eq!(s.follow_link(l.id,&a).unwrap(),LinkState::Unavailable);
    s.move_entity(n.id,0,None,&auth).unwrap();assert!(s.entity(n.id,&a).is_err());assert!(s.entity(n.id,&auth).unwrap().locked);
    s.set_app_lock(true,&auth).unwrap();assert!(s.create(Kind::Note,None,"denied",&a).is_err());assert!(s.list(None,Order::Name,None,None,&a).is_err());
}
#[test]
fn chronology_filters_by_actual_creation_without_journals(){
    let mut s=Store::memory().unwrap();let a=Access::public();let z=s.create(Kind::Note,None,"z",&a).unwrap();s.create(Kind::Note,None,"a",&a).unwrap();let list=s.list(None,Order::Name,None,None,&a).unwrap();assert_eq!(list[0].title,"a");assert!(s.list(None,Order::Created,Some(z.created_ms+60_000),None,&a).unwrap().is_empty());
}
