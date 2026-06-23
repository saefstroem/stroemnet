use alloy::sol;

sol! {
    #[sol(rpc)]
    contract StroemHTLCV1 {
        function swaps(bytes32 swapId)
            external
            view
            returns (
                address sender,
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
