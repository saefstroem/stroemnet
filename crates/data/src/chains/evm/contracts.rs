use alloy::sol;

sol! {
    #[sol(rpc)]
    contract StroemHTLCV1 {
        function swaps(bytes32 swapId)
            external
            view
            returns (
                address sender,
                bytes sender_destination_address,
                address receiver,
                uint256 amount,
                bytes32 secretHash,
                uint256 timelock,
                bool initialized,
                bool finalized
            );

        event Commitment(
            bytes32 indexed swapId,
            address indexed sender,
            address indexed receiver,
            bytes sender_destination_address,
            uint256 amount,
            bytes32 secretHash,
            uint256 timelock,
            uint8 destination
        );

        event Refund(bytes32 indexed swapId);
        event Claim(
            bytes32 indexed swapId,
            bytes32 secret,
            address indexed solver,
            uint256 receiverAmount,
            uint256 solverReward
        );
        event SolverReward(bytes32 indexed swapId, address indexed solver, uint256 reward);

        function newSwap(
            address onBehalfOf,
            address _receiver,
            bytes calldata _senderDestination,
            bytes32 _secretHash,
            uint256 _timelock,
            uint8 _destination,
            bytes32 swapId
        ) external payable returns (bytes32 swapId);

        function claim(bytes32 _swapId, bytes32 _secret) external;

        function refund(bytes32 _swapId) external;
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::StroemHTLCV1;
    use alloy::primitives::{Address, B256, Bytes, U256};
    use alloy::sol_types::{SolCall, SolValue};

    #[test]
    fn swaps_return_matches_eight_field_solidity_struct() {
        let encoded = (
            Address::repeat_byte(0x11),
            Bytes::from(vec![0xAA, 0xBB, 0xCC]),
            Address::repeat_byte(0x22),
            U256::from(1000u64),
            B256::repeat_byte(0x33),
            U256::from(1_700_000_000u64),
            true,
            false,
        )
            .abi_encode_params();

        let ret = StroemHTLCV1::swapsCall::abi_decode_returns(&encoded).unwrap();

        assert_eq!(ret.sender, Address::repeat_byte(0x11));
        assert_eq!(
            ret.sender_destination_address,
            Bytes::from(vec![0xAA, 0xBB, 0xCC])
        );
        assert_eq!(ret.receiver, Address::repeat_byte(0x22));
        assert_eq!(ret.amount, U256::from(1000u64));
        assert_eq!(ret.secretHash, B256::repeat_byte(0x33));
        assert_eq!(ret.timelock, U256::from(1_700_000_000u64));
        assert!(ret.initialized);
        assert!(!ret.finalized);
    }
}
