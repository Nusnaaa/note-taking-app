#![allow(dead_code)]
use std::collections::BTreeSet;
use stem_core::{
    ink::{Point, Stroke, Tool},
    security::{Access, Authenticator, Scope},
    Id,
};

pub fn stroke(sequence: u64) -> Stroke {
    Stroke {
        id: Id::new(),
        capture_session: Id::new(),
        sequence,
        pointer_id: 1,
        coordinate_space: "synthetic-local-units".into(),
        tool: Tool::Pen,
        style: None,
        end: Default::default(),
        points: vec![
            Point {
                x: 0.1,
                y: -0.0,
                time_us: 10,
                pressure: None,
                tilt_rad: None,
                orientation_rad: None,
                buttons: None,
            },
            Point {
                x: 1.2345678901234567,
                y: 2.0,
                time_us: 20,
                pressure: Some(0.),
                tilt_rad: Some(0.3),
                orientation_rad: Some(-0.1),
                buttons: Some(0),
            },
        ],
    }
}
pub struct TestAuth(pub bool);
impl Authenticator for TestAuth {
    fn verify(&mut self, _: Id, _: &BTreeSet<Scope>) -> bool {
        self.0
    }
}
pub fn access(scopes: impl IntoIterator<Item = Scope>) -> Access {
    Access::authenticate(&mut TestAuth(true), scopes.into_iter().collect()).unwrap()
}
