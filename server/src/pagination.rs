use serde::Serialize;

use crate::error::ApiError;

const DEFAULT_PER_PAGE: u32 = 20;
const MAX_PER_PAGE: u32 = 100;

#[derive(Debug, Clone, Copy)]
pub struct PageRequest {
    pub page: u32,
    pub per_page: u32,
}

impl PageRequest {
    pub fn parse(page: Option<&str>, per_page: Option<&str>) -> Result<Self, ApiError> {
        let page = match page {
            None => 1,
            Some(text) => text
                .parse()
                .ok()
                .filter(|page| *page >= 1)
                .ok_or_else(|| ApiError::bad_request("invalid_query", "page must be a number from 1"))?,
        };
        let per_page = match per_page {
            None => DEFAULT_PER_PAGE,
            Some(text) => text
                .parse()
                .ok()
                .filter(|per_page| (1..=MAX_PER_PAGE).contains(per_page))
                .ok_or_else(|| {
                    ApiError::bad_request(
                        "invalid_query",
                        format!("per_page must be a number from 1 to {MAX_PER_PAGE}"),
                    )
                })?,
        };
        Ok(Self { page, per_page })
    }

    pub fn limit(self) -> i64 {
        i64::from(self.per_page)
    }

    pub fn offset(self) -> i64 {
        i64::from(self.page - 1) * i64::from(self.per_page)
    }

    pub fn page_of<T>(self, items: Vec<T>, total: u64) -> Page<T> {
        Page { items, page: self.page, per_page: self.per_page, total }
    }
}

#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
}

/// A list short enough to be returned whole.
#[derive(Debug, Serialize)]
pub struct Items<T> {
    pub items: Vec<T>,
}
