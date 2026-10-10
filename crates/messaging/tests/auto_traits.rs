//! Compile-time contracts for owned deliveries, errors, and borrowed port futures.

use edgeagent_messaging::{
    ClaimedMessage, ConsumeError, HandlerFailure, InboundProcessingError, InboxDisposition,
    InboxFuture, InboxStoreError, MessageDelivery, OutboxStoreError, OutboxStoreFuture,
    OutboxTimingError, PublishError, PublishFuture, QuarantineDisposition, ReceiveFuture,
    SettlementFuture,
};

fn assert_send<T: Send>() {}

fn assert_send_sync_static<T: Send + Sync + 'static>() {}

fn assert_borrowed_futures_are_send<'operation>(_scope: &'operation u8) {
    // These aliases must be sendable for any operation lifetime, not only
    // 'static futures. Sending an owned delivery does not require it to be Sync.
    assert_send::<ReceiveFuture<'operation>>();
    assert_send::<PublishFuture<'operation>>();
    assert_send::<InboxFuture<'operation, Result<InboxDisposition, InboundProcessingError>>>();
    assert_send::<InboxFuture<'operation, Result<QuarantineDisposition, InboxStoreError>>>();
    assert_send::<OutboxStoreFuture<'operation, Option<ClaimedMessage>>>();
    assert_send::<OutboxStoreFuture<'operation, ()>>();
}

#[test]
fn owned_delivery_and_every_port_future_remain_sendable() {
    assert_send::<MessageDelivery>();
    assert_send::<SettlementFuture>();
    let operation_scope = 0;
    assert_borrowed_futures_are_send(&operation_scope);
}

#[test]
fn public_messaging_errors_remain_send_sync_and_static() {
    assert_send_sync_static::<ConsumeError>();
    assert_send_sync_static::<PublishError>();
    assert_send_sync_static::<InboxStoreError>();
    assert_send_sync_static::<OutboxStoreError>();
    assert_send_sync_static::<HandlerFailure>();
    assert_send_sync_static::<InboundProcessingError>();
    assert_send_sync_static::<OutboxTimingError>();
}
