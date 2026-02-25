use serde::{Deserialize, Serialize};
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

// ── Query params ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<u32>,
    pub cursor: Option<Uuid>,
    pub direction: Option<String>,
}

impl PaginationQuery {
    pub fn into_params(self) -> PaginationParams {
        let limit = self.limit.unwrap_or(20).min(100);
        let direction = match self.direction.as_deref() {
            Some("backward") => PaginationDirection::Backward,
            _ => PaginationDirection::Forward,
        };
        PaginationParams {
            limit,
            cursor: self.cursor,
            direction,
        }
    }
}

// ── JSON response wrapper ───────────────────────────────────

#[derive(Debug, Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
