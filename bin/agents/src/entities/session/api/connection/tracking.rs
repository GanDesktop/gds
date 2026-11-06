use super::*;

/// Reads only the accepted turn's authoritative outcome, independently of the
/// selected conversation. Reconnects resume after the last decoded event.
///
/// # Errors
/// Returns an error if history is unavailable, malformed, or ends without the
/// accepted turn's outcome.
pub(super) async fn track(
    mut client: Client,
    session: SessionId,
    receipt: TurnReceipt,
) -> Result<Event, Failure> {
    let mut cursor = receipt.cursor.raw();
    let mut management_cursor = cursor;
    loop {
        let response = tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.observe_acp(pb::AttachRequest {
                session: session.to_string(),
                cursor: Some(cursor),
                replay: 0,
            }),
        )
        .await;
        let mut stream = match response {
            Ok(Ok(response)) => response.into_inner(),
            Ok(Err(error)) if !retryable(&error) => return Err(error.into()),
            _ => {
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue;
            }
        };
        let mut management = match tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.attach(pb::AttachRequest {
                session: session.to_string(),
                cursor: Some(management_cursor),
                replay: 0,
            }),
        )
        .await
        {
            Ok(Ok(response)) => response.into_inner(),
            Ok(Err(error)) if !retryable(&error) => return Err(error.into()),
            _ => {
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue;
            }
        };
        let mut management_open = true;
        let mut protocol_open = true;
        loop {
            if !protocol_open && !management_open {
                return Err(Failure::Condition(Condition::TrackingEnded));
            }
            let next = tokio::select! {
                message = stream.message(), if protocol_open => message,
                message = management.message(), if management_open => {
                    match message {
                        Ok(Some(raw)) => {
                            let event = Event::try_from(raw).map_err(|error| Failure::InvalidPayload(error.to_string()))?;
                            if event.session != session { return Err(Failure::Condition(Condition::TrackingWrongSession)); }
                            management_cursor = management_cursor.max(event.cursor.raw());
                            if matches!(event.kind, EventKind::TurnInterrupted { turn, .. } if turn == receipt.turn) { return Ok(event); }
                            continue;
                        }
                        Ok(None) => { management_open = false; continue; }
                        Err(error) if !retryable(&error) => return Err(error.into()),
                        Err(_) => { tokio::time::sleep(RECONNECT_DELAY).await; break; }
                    }
                }
            };
            match next {
                Ok(Some(event)) => {
                    if event.session != session.to_string() {
                        return Err(Failure::Condition(Condition::TrackingWrongSession));
                    }
                    if event.cursor == 0 || event.cursor <= cursor {
                        continue;
                    }
                    cursor = event.cursor;
                    for event in event
                        .history_events()
                        .map_err(|error| Failure::InvalidPayload(error.to_string()))?
                    {
                        if matches!(event.kind, EventKind::TurnFinished { turn, .. } if turn == receipt.turn)
                        {
                            return Ok(event);
                        }
                    }
                }
                Ok(None) => {
                    protocol_open = false;
                }
                Err(error) if !retryable(&error) => return Err(error.into()),
                Err(_) => {
                    tokio::time::sleep(RECONNECT_DELAY).await;
                    break;
                }
            }
        }
    }
}

fn retryable(error: &tonic::Status) -> bool {
    matches!(error.code(), Code::Unavailable | Code::DeadlineExceeded)
}
