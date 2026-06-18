use stroemnet_p2p::wire::message::{P2pMsg, ProposalRequest};
use stroemnet_p2p::wire::{decode, encode};
use stroemnet_test_harness::test_handler;

#[tokio::test]
async fn proposal_request_roundtrip_codec() {
    let req = ProposalRequest {
        swap_id: [42; 32],
        origin: 1,
        destination: 0,
        amount: "1000000000000000000".into(),
        extra_data: vec![],
    };
    let bytes = encode(&P2pMsg::ProposalRequest(req.clone())).unwrap();
    let decoded = decode(&bytes).unwrap();
    assert_eq!(decoded, P2pMsg::ProposalRequest(req));
}

#[tokio::test]
async fn handler_creates_proposal_for_p2p_request() {
    use stroemnet_handler::handle::proposal::SwapRequest;
    use stroemnet_protocol::ChannelId;

    let handler = test_handler();
    handler.price_storage.set(ChannelId::KaspaTn10, 0.15);
    handler
        .price_storage
        .set(ChannelId::EthereumSepolia, 3000.0);

    let p2p_req = ProposalRequest {
        swap_id: [7; 32],
        origin: ChannelId::EthereumSepolia as u8,
        destination: ChannelId::KaspaTn10 as u8,
        amount: "1000000000000000000".into(),
        extra_data: vec![],
    };
    let request = SwapRequest {
        origin: p2p_req.origin,
        destination: p2p_req.destination,
        amount: p2p_req.amount.clone(),
    };

    let proposal = handler.create_proposal(&request).await.unwrap();
    assert_eq!(proposal.origin, ChannelId::KaspaTn10);
    assert_eq!(proposal.destination, ChannelId::EthereumSepolia);
    assert!(proposal.amount_in != "0");
}
