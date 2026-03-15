use serde::{Deserialize, Serialize};
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

// ── Query params ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<u32>,
    pub cursor: Option<Uuid>,
    pub direction: Option<PaginationDirection>,
}

impl PaginationQuery {
    pub fn into_params(self) -> PaginationParams {
        PaginationParams {
            limit: self.limit.unwrap_or(20).min(100),
            cursor: self.cursor,
            direction: self.direction.unwrap_or(PaginationDirection::Forward),
        }
    }
}

// ── JSON response wrapper ───────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
}

impl<T: Serialize> PaginatedResponse<T> {
    pub fn new(res: PaginationRes<T>) -> Self {
        Self {
            items: res.items,
            next_cursor: res.next_cursor.map(|id| id.to_string()),
            prev_cursor: res.prev_cursor.map(|id| id.to_string()),
        }
    }
}

// ── Core types ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaginationDirection {
    Forward,
    Backward,
}

pub trait HasId {
    fn id(&self) -> Uuid;
}

pub struct NextAndPrevCursor {
    pub next_cursor: Option<Uuid>,
    pub prev_cursor: Option<Uuid>,
}

pub struct PaginationRes<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<Uuid>,
    pub prev_cursor: Option<Uuid>,
}

impl<T> PaginationRes<T> {
    pub fn new(items: Vec<T>, cursors: NextAndPrevCursor) -> Self {
        Self {
            items,
            next_cursor: cursors.next_cursor,
            prev_cursor: cursors.prev_cursor,
        }
    }
}

#[derive(Clone)]
pub struct PaginationParams {
    pub limit: u32,
    pub cursor: Option<Uuid>,
    pub direction: PaginationDirection,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            limit: 20,
            cursor: None,
            direction: PaginationDirection::Forward,
        }
    }
}

pub fn keyset_paginate(
    params: &PaginationParams,
    alias: Option<&str>,
    qb: &mut QueryBuilder<Postgres>,
) {
    let id_column = if let Some(alias) = alias {
        format!("{}.id", alias)
    } else {
        "id".to_string()
    };

    match params.direction {
        PaginationDirection::Forward => {
            if let Some(last_id) = params.cursor {
                qb.push(format!(" AND {} > ", id_column).as_str());
                qb.push_bind(last_id);
            }
            qb.push(format!(" ORDER BY {} ASC LIMIT ", id_column).as_str());
        }
        PaginationDirection::Backward => {
            if let Some(first_id) = params.cursor {
                qb.push(format!(" AND {} < ", id_column).as_str());
                qb.push_bind(first_id);
            }
            qb.push(format!(" ORDER BY {} DESC LIMIT ", id_column).as_str());
        }
    }
    qb.push_bind((params.limit + 1) as i64);
}

pub fn get_cursors<T: HasId>(params: &PaginationParams, rows: &mut Vec<T>) -> NextAndPrevCursor {
    let has_more = rows.len() > params.limit as usize;
    if has_more {
        rows.pop();
    }

    if matches!(params.direction, PaginationDirection::Backward) {
        rows.reverse();
    }

    let start_id = rows.first().map(|r| r.id());
    let end_id = rows.last().map(|r| r.id());

    let (next_cursor, prev_cursor) = match params.direction {
        PaginationDirection::Forward => {
            let next = if has_more { end_id } else { None };
            let prev = if params.cursor.is_some() {
                start_id
            } else {
                None
            };
            (next, prev)
        }
        PaginationDirection::Backward => {
            let next = end_id;
            let prev = if has_more { start_id } else { None };
            (next, prev)
        }
    };

    NextAndPrevCursor {
        next_cursor,
        prev_cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct Row(Uuid);
    impl HasId for Row {
        fn id(&self) -> Uuid {
            self.0
        }
    }

    fn make_rows(n: usize) -> Vec<Row> {
        (0..n).map(|_| Row(Uuid::now_v7())).collect()
    }

    // ── Forward pagination ─────────────────────────────────

    #[test]
    fn forward_empty_results() {
        let params = PaginationParams {
            limit: 10,
            ..Default::default()
        };
        let mut rows: Vec<Row> = vec![];
        let cursors = get_cursors(&params, &mut rows);

        assert!(rows.is_empty());
        assert!(cursors.next_cursor.is_none());
        assert!(cursors.prev_cursor.is_none());
    }

    #[test]
    fn forward_fewer_than_limit_no_cursors() {
        let params = PaginationParams {
            limit: 10,
            ..Default::default()
        };
        let mut rows = make_rows(5);
        let cursors = get_cursors(&params, &mut rows);

        assert_eq!(rows.len(), 5);
        assert!(cursors.next_cursor.is_none());
        assert!(cursors.prev_cursor.is_none());
    }

    #[test]
    fn forward_exactly_limit_no_next() {
        let params = PaginationParams {
            limit: 5,
            ..Default::default()
        };
        let mut rows = make_rows(5); // DB returned exactly limit (no extra)
        let cursors = get_cursors(&params, &mut rows);

        assert_eq!(rows.len(), 5);
        assert!(cursors.next_cursor.is_none());
        assert!(cursors.prev_cursor.is_none());
    }

    #[test]
    fn forward_more_than_limit_has_next() {
        let params = PaginationParams {
            limit: 5,
            ..Default::default()
        };
        let mut rows = make_rows(6); // limit+1 means there's a next page
        let last_kept = rows[4].0;
        let cursors = get_cursors(&params, &mut rows);

        assert_eq!(rows.len(), 5); // extra row trimmed
        assert_eq!(cursors.next_cursor, Some(last_kept));
        assert!(cursors.prev_cursor.is_none()); // first page
    }

    #[test]
    fn forward_with_cursor_has_prev() {
        let cursor_id = Uuid::now_v7();
        let params = PaginationParams {
            limit: 5,
            cursor: Some(cursor_id),
            direction: PaginationDirection::Forward,
        };
        let mut rows = make_rows(3);
        let first_id = rows[0].0;
        let cursors = get_cursors(&params, &mut rows);

        assert_eq!(rows.len(), 3);
        assert!(cursors.next_cursor.is_none()); // last page
        assert_eq!(cursors.prev_cursor, Some(first_id));
    }

    #[test]
    fn forward_with_cursor_has_both() {
        let cursor_id = Uuid::now_v7();
        let params = PaginationParams {
            limit: 3,
            cursor: Some(cursor_id),
            direction: PaginationDirection::Forward,
        };
        let mut rows = make_rows(4); // limit+1
        let first_id = rows[0].0;
        let last_kept = rows[2].0;
        let cursors = get_cursors(&params, &mut rows);

        assert_eq!(rows.len(), 3);
        assert_eq!(cursors.next_cursor, Some(last_kept));
        assert_eq!(cursors.prev_cursor, Some(first_id));
    }

    // ── Backward pagination ────────────────────────────────

    #[test]
    fn backward_fewer_than_limit() {
        let params = PaginationParams {
            limit: 10,
            cursor: Some(Uuid::now_v7()),
            direction: PaginationDirection::Backward,
        };
        let mut rows = make_rows(3);
        // backward reverses rows
        let cursors = get_cursors(&params, &mut rows);

        assert_eq!(rows.len(), 3);
        assert!(cursors.next_cursor.is_some()); // backward always has next (end_id)
        assert!(cursors.prev_cursor.is_none()); // no more backward pages
    }

    #[test]
    fn backward_more_than_limit_has_prev() {
        let params = PaginationParams {
            limit: 3,
            cursor: Some(Uuid::now_v7()),
            direction: PaginationDirection::Backward,
        };
        let mut rows = make_rows(4); // limit+1
        let cursors = get_cursors(&params, &mut rows);

        assert_eq!(rows.len(), 3); // trimmed
        assert!(cursors.next_cursor.is_some());
        assert!(cursors.prev_cursor.is_some()); // more backward pages
    }

    // ── No-overlap guarantee ───────────────────────────────

    #[test]
    fn forward_pages_do_not_overlap() {
        let all_rows = make_rows(7);
        let all_ids: Vec<Uuid> = all_rows.iter().map(|r| r.0).collect();

        // Page 1
        let params = PaginationParams {
            limit: 3,
            ..Default::default()
        };
        let mut page1: Vec<Row> = all_ids[0..4].iter().map(|&id| Row(id)).collect();
        let c1 = get_cursors(&params, &mut page1);
        let page1_ids: Vec<Uuid> = page1.iter().map(|r| r.0).collect();
        assert_eq!(page1_ids.len(), 3);
        assert!(c1.next_cursor.is_some());

        // Page 2 (using cursor)
        let params = PaginationParams {
            limit: 3,
            cursor: c1.next_cursor,
            direction: PaginationDirection::Forward,
        };
        let mut page2: Vec<Row> = all_ids[3..7].iter().map(|&id| Row(id)).collect();
        let c2 = get_cursors(&params, &mut page2);
        let page2_ids: Vec<Uuid> = page2.iter().map(|r| r.0).collect();

        // No overlap
        for id in &page2_ids {
            assert!(!page1_ids.contains(id), "Pages overlap on id {}", id);
        }
        assert_eq!(page2_ids.len(), 3);
        assert!(c2.next_cursor.is_some()); // still has next page (1 remaining)
    }
}
