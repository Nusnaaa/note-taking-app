//! Derived navigation/search projections never duplicate authored content.
use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub master: Id,
    pub block: Option<Id>,
    pub exercise: Option<Id>,
}
impl Store {
    pub fn flat_exercises(&self, access: &Access) -> Result<Vec<Exercise>> {
        Self::require_app(&self.db, access)?;
        let mut q = self.db.prepare("SELECT id FROM exercises ORDER BY rowid")?;
        let mut values = vec![];
        for row in q.query_map([], |r| r.get::<_, String>(0))? {
            match self.exercise(Id::parse(&row?)?, access) {
                Ok(e) if e.visible => values.push(e),
                Ok(_) | Err(Error::Denied | Error::Unavailable) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(values)
    }
    pub fn search_views(&self, query: &str, access: &Access) -> Result<Vec<SearchHit>> {
        let mut hits = vec![];
        for (master, block) in self.search(query, access)? {
            let exercise = block
                .map(|b| Self::block_exercise(&self.db, b))
                .transpose()?
                .flatten();
            let exercise = exercise.filter(|e| self.exercise(*e, access).is_ok_and(|e| e.visible));
            hits.push(SearchHit {
                master,
                block,
                exercise,
            });
        }
        for e in self.flat_exercises(access)? {
            if e.title.contains(query) && !hits.iter().any(|h| h.exercise == Some(e.id)) {
                hits.push(SearchHit {
                    master: e.master,
                    block: e.heading,
                    exercise: Some(e.id),
                });
            }
        }
        Ok(hits)
    }
}
