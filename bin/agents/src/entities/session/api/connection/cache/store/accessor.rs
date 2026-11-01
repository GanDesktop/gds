use bas::{
    model::{Event, SessionId},
    service::HistoryWindow,
};
use gds::service::storage::Transaction;
use serde::Serialize;
use toasty::update;

use super::super::{Error, record};

const ROW_OVERHEAD: i64 = 256;

pub(super) struct Accessor<'tx, 'db> {
    tx: &'tx mut Transaction<'db>,
    namespace: &'tx str,
    limit: i64,
}

impl<'tx, 'db> Accessor<'tx, 'db> {
    pub(super) fn new(tx: &'tx mut Transaction<'db>, namespace: &'tx str, limit: i64) -> Self {
        Self {
            tx,
            namespace,
            limit,
        }
    }

    /// # Errors
    /// Reports database failures.
    pub(super) async fn remove(&mut self, id: SessionId) -> Result<(), Error> {
        self.delete(self.namespace, &id.to_string()).await
    }

    /// # Errors
    /// Reports database failures or corrupt history.
    pub(super) async fn window(
        &mut self,
        id: SessionId,
        before: Option<u64>,
    ) -> Result<Option<HistoryWindow>, Error> {
        let Some(session) = self.session(id).await? else {
            return Ok(None);
        };
        let requested = before.map(|before| before - 1);
        let pages = record::Page::filter(record::Page::fields().namespace().eq(self.namespace))
            .filter(record::Page::fields().session().eq(id.to_string()));
        let page = match requested {
            Some(upper) => {
                pages
                    .filter(record::Page::fields().upper().ge(raw(upper)?))
                    .filter(
                        record::Page::fields()
                            .upper()
                            .eq(raw(upper)?)
                            .or(record::Page::fields().start().le(raw(upper)?)),
                    )
                    .order_by(record::Page::fields().upper().asc())
                    .first()
                    .exec(self.tx)
                    .await?
            }
            None => {
                pages
                    .order_by(record::Page::fields().upper().desc())
                    .first()
                    .exec(self.tx)
                    .await?
            }
        };
        let Some(page) = page else {
            return Ok(None);
        };
        let upper = requested.unwrap_or(positive(session.frontier)?);
        if page.upper > session.frontier {
            return Err(Error::Invalid("page exceeds frontier"));
        }
        let rows = record::Event::filter(record::Event::fields().namespace().eq(self.namespace))
            .filter(record::Event::fields().session().eq(id.to_string()))
            .filter(record::Event::fields().cursor().ge(page.start))
            .filter(record::Event::fields().cursor().le(raw(upper)?))
            .order_by(record::Event::fields().cursor().asc())
            .exec(self.tx)
            .await?;
        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let event = row.payload.0;
            if event.session != id || event.cursor.raw() != positive(row.cursor)? {
                return Err(Error::Invalid("event key mismatch"));
            }
            events.push(event);
        }
        let window = HistoryWindow {
            cursor: if requested.is_some() {
                session.snapshot.meta.revision.raw()
            } else {
                upper
            },
            session: session.snapshot.0,
            events,
            context: page.context.0,
            before: page.before.map(positive).transpose()?,
        };
        validate(&window)?;
        if window.events.last().map_or(0, |event| event.cursor.raw()) != upper {
            return Err(Error::Invalid("missing retained events at page end"));
        }
        if window
            .events
            .first()
            .map(|event| event.cursor.raw())
            .unwrap_or(upper.saturating_add(1))
            != positive(page.start)?
        {
            return Err(Error::Invalid("missing retained events"));
        }
        self.touch(id).await?;
        Ok(Some(window))
    }

    /// # Errors
    /// Reports invalid history, database or serialization failures.
    pub(super) async fn store_window(&mut self, window: &HistoryWindow) -> Result<(), Error> {
        validate(window)?;
        let id = window.session.meta.id;
        let upper = window.events.last().map_or(0, |event| event.cursor.raw());
        let existing = self.session(id).await?;
        let frontier = existing
            .as_ref()
            .map_or(raw(upper)?, |old| old.frontier.max(upper as i64));
        let snapshot = match existing.as_ref() {
            Some(old) if old.snapshot.meta.revision > window.session.meta.revision => {
                old.snapshot.0.clone()
            }
            _ => window.session.clone(),
        };
        let snapshot_bytes = encoded_bytes(&snapshot)?;
        let mut bytes = existing
            .as_ref()
            .map_or(ROW_OVERHEAD, |old| old.bytes - old.snapshot_bytes)
            + snapshot_bytes;
        record::Session::upsert_by_namespace_and_session(self.namespace, id.to_string())
            .snapshot(snapshot)
            .frontier(frontier)
            .snapshot_bytes(snapshot_bytes)
            .bytes(bytes)
            .exec(self.tx)
            .await?;
        for event in &window.events {
            bytes += self.insert_event(event).await?;
        }
        let prior = record::Page::filter_by_namespace_and_session_and_upper(
            self.namespace,
            id.to_string(),
            raw(upper)?,
        )
        .first()
        .exec(self.tx)
        .await?;
        let start = window
            .events
            .first()
            .map_or(upper.saturating_add(1), |event| event.cursor.raw());
        let (start, before, context) = match prior.as_ref() {
            Some(saved) if saved.start < raw(start)? => {
                (saved.start, saved.before, saved.context.0.clone())
            }
            _ => (
                raw(start)?,
                window.before.map(raw).transpose()?,
                window.context.clone(),
            ),
        };
        let page_bytes = encoded_bytes(&context)? + ROW_OVERHEAD;
        bytes += page_bytes - prior.as_ref().map_or(0, |saved| saved.bytes);
        record::Page::upsert_by_namespace_and_session_and_upper(
            self.namespace,
            id.to_string(),
            raw(upper)?,
        )
        .start(start)
        .before(before)
        .context(context)
        .bytes(page_bytes)
        .exec(self.tx)
        .await?;
        update!(record::Session::filter_by_namespace_and_session(self.namespace, id.to_string()) { bytes })
            .exec(self.tx).await?;
        self.touch(id).await?;
        self.evict(Some(id)).await
    }

    /// # Errors
    /// Reports mismatched sessions, database or serialization failures.
    pub(super) async fn append_events(
        &mut self,
        id: SessionId,
        events: &[Event],
    ) -> Result<(), Error> {
        let Some(session) = self.session(id).await? else {
            return Ok(());
        };
        let mut frontier = positive(session.frontier)?;
        let mut snapshot = session.snapshot.0;
        let mut bytes = session.bytes - session.snapshot_bytes;
        for event in events {
            if event.cursor.raw() == 0 || event.is_transient() {
                continue;
            }
            if event.session != id {
                return Err(Error::Invalid("append belongs to another session"));
            }
            if event.cursor.raw() <= frontier {
                continue;
            }
            if frontier.checked_add(1) != Some(event.cursor.raw()) {
                break;
            }
            bytes += self.insert_event(event).await?;
            snapshot.meta.apply_event(event);
            frontier = event.cursor.raw();
        }
        let snapshot_bytes = encoded_bytes(&snapshot)?;
        bytes += snapshot_bytes;
        update!(record::Session::filter_by_namespace_and_session(self.namespace, id.to_string()) {
            snapshot, frontier: raw(frontier)?, snapshot_bytes, bytes,
        })
        .exec(self.tx)
        .await?;
        self.touch(id).await?;
        self.evict(Some(id)).await
    }

    async fn session(&mut self, id: SessionId) -> Result<Option<record::Session>, Error> {
        let session =
            record::Session::filter_by_namespace_and_session(self.namespace, id.to_string())
                .first()
                .exec(self.tx)
                .await?;
        if let Some(session) = &session
            && (session.snapshot.meta.id != id
                || session.snapshot.meta.revision.raw() < positive(session.frontier)?)
        {
            return Err(Error::Invalid("snapshot key or revision mismatch"));
        }
        Ok(session)
    }

    async fn insert_event(&mut self, event: &Event) -> Result<i64, Error> {
        let bytes = encoded_bytes(event)? + ROW_OVERHEAD;
        let inserted = record::Event::upsert_by_namespace_and_session_and_cursor(
            self.namespace,
            event.session.to_string(),
            raw(event.cursor.raw())?,
        )
        .payload(event.clone())
        .bytes(bytes)
        .or_ignore()
        .exec(self.tx)
        .await?;
        Ok(if inserted.is_some() { bytes } else { 0 })
    }

    async fn touch(&mut self, id: SessionId) -> Result<(), Error> {
        let last = record::Session::all()
            .order_by(record::Session::fields().touched().desc())
            .select(record::Session::fields().touched())
            .first()
            .exec(self.tx)
            .await?
            .unwrap_or(0);
        update!(record::Session::filter_by_namespace_and_session(self.namespace, id.to_string()) { touched: last + 1 })
            .exec(self.tx).await?;
        Ok(())
    }

    /// # Errors
    /// Reports database failures or invalid cache accounting.
    pub(super) async fn evict(&mut self, written: Option<SessionId>) -> Result<(), Error> {
        if let Some(id) = written {
            let bytes =
                record::Session::filter_by_namespace_and_session(self.namespace, id.to_string())
                    .select(record::Session::fields().bytes())
                    .one()
                    .exec(self.tx)
                    .await?;
            if bytes > self.limit {
                self.remove(id).await?;
            }
        }
        let mut total: i64 = record::Session::all()
            .select(record::Session::fields().bytes())
            .exec(self.tx)
            .await?
            .into_iter()
            .sum();
        while total > self.limit {
            let oldest = record::Session::all()
                .order_by(record::Session::fields().touched().asc())
                .first()
                .exec(self.tx)
                .await?
                .ok_or(Error::Invalid("missing eviction candidate"))?;
            self.delete(&oldest.namespace, &oldest.session).await?;
            total -= oldest.bytes;
        }
        Ok(())
    }

    async fn delete(&mut self, namespace: &str, session: &str) -> Result<(), Error> {
        record::Event::filter(record::Event::fields().namespace().eq(namespace))
            .filter(record::Event::fields().session().eq(session))
            .delete()
            .exec(self.tx)
            .await?;
        record::Page::filter(record::Page::fields().namespace().eq(namespace))
            .filter(record::Page::fields().session().eq(session))
            .delete()
            .exec(self.tx)
            .await?;
        record::Session::filter_by_namespace_and_session(namespace, session)
            .delete()
            .exec(self.tx)
            .await?;
        Ok(())
    }
}

fn encoded_bytes(value: &impl Serialize) -> Result<i64, Error> {
    raw(serde_json::to_vec(value)?.len() as u64)
}

fn validate(window: &HistoryWindow) -> Result<(), Error> {
    raw(window.cursor)?;
    if window.cursor > window.session.meta.revision.raw() {
        return Err(Error::Invalid("window exceeds metadata revision"));
    }
    let mut previous = None;
    for event in &window.events {
        if event.session != window.session.meta.id
            || event.cursor.raw() == 0
            || event.is_transient()
            || previous.is_some_and(|cursor: u64| cursor.checked_add(1) != Some(event.cursor.raw()))
        {
            return Err(Error::Invalid(
                "window is not contiguous replayable history",
            ));
        }
        previous = Some(event.cursor.raw());
    }
    if previous.is_some_and(|last| last > window.cursor) {
        return Err(Error::Invalid("window exceeds its snapshot"));
    }
    let start = window.events.first().map_or(1, |event| event.cursor.raw());
    if window
        .before
        .is_some_and(|before| before != start || before == 0)
        || window.context.iter().any(|event| {
            event.session != window.session.meta.id
                || event.cursor.raw() == 0
                || event.cursor.raw() >= start
                || event.is_transient()
        })
    {
        return Err(Error::Invalid("invalid context or older-page boundary"));
    }
    Ok(())
}

fn raw(value: u64) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::Invalid("cursor exceeds SQLite range"))
}

fn positive(value: i64) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::Invalid("negative cursor"))
}
