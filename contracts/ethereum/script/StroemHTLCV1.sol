// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract StroemHTLCV1 {
    struct Swap {
        address sender;
        bytes sender_destination_address;
        address receiver;
        uint256 amount;
        bytes32 secretHash;
        uint256 timelock;
        bool initialized;
        bool finalized;
    }

    mapping(bytes32 => Swap) public swaps;
    mapping(bytes32 => bool) public secretHashes;

    /// @notice Solver reward: 0.1% of the swap amount (1/1000)
    uint256 public constant BPS_DENOMINATOR = 1000;

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
        bytes32 indexed  ,
        bytes32 secret,
        address indexed solver,
        uint256 receiverAmount,
        uint256 solverReward
    );
    event SolverReward(bytes32 indexed swapId, address indexed solver, uint256 reward);

    /**
     * @notice Create a new HTLC swap
     * @param _receiver Address that receives funds when secret is revealed
     * @param _senderDestination Bytes representing the sender's destination on the other chain (e.g. Bitcoin address, Kaspa address, etc.)
     * @param _secretHash SHA256 hash of the secret
     * @param _timelock Unix timestamp after which refund is possible
     * @param _destination Enum value representing the destination chain (e.g. 1 for Bitcoin, 2 for Kaspa, etc.)
     * @param swapId Swap ID used to identify the swap.
     */
    function newSwap(
        address onBehalfOf,
        address _receiver,
        bytes calldata _senderDestination,
        bytes32 _secretHash,
        uint256 _timelock,
        uint8 _destination,
        bytes32 swapId
    ) external payable {
        require(msg.value > BPS_DENOMINATOR, "Amount must be greater than 0");
        require(_timelock > block.timestamp, "Timelock must be in the future");
        require(_receiver != address(0), "Invalid receiver address");
        require(_secretHash != bytes32(0), "Invalid secret hash");
        require(swaps[swapId].initialized == false, "Swap ID already exists");
        require(swapId != bytes32(0), "Invalid swap ID");
        require(!secretHashes[_secretHash], "Secret hash already used");
        secretHashes[_secretHash] = true;

        swaps[swapId] = Swap({
            sender: onBehalfOf,
            sender_destination_address: _senderDestination,
            receiver: _receiver,
            amount: msg.value,
            secretHash: _secretHash,
            timelock: _timelock,
            initialized: true,
            finalized: false
        });

        emit Commitment(
            swapId,
            onBehalfOf,
            _receiver,
            _senderDestination,
            msg.value,
            _secretHash,
            _timelock,
            _destination
        );
    }

    /**
     * @notice Claim funds by revealing the secret (CCR path)
     * @param _swapId Swap identifier
     * @param _secret Preimage of secretHash
     * @dev Anyone can call this. Solver (msg.sender) gets 0.1% reward.
     *      Receiver gets amount - reward. No funds remain in contract.
     *      solverReward = amount / 1000 (integer division, floors)
     *      receiverAmount = amount - solverReward (gets the remainder)
     */
    function claim(bytes32 _swapId, bytes32 _secret) external {
        Swap storage swap = swaps[_swapId];
        require(swap.initialized, "Swap does not exist");
        require(!swap.finalized, "Already finalized");
        require(sha256(abi.encodePacked(_secret)) == swap.secretHash, "Invalid secret");
        require(block.timestamp < swap.timelock, "Timelock expired");

        swap.finalized = true;

        uint256 solverReward = swap.amount / BPS_DENOMINATOR;
        uint256 receiverAmount = swap.amount - solverReward;

        emit Claim(_swapId, _secret, msg.sender, receiverAmount, solverReward);

        // Transfer to receiver first (the larger amount)
        (bool successReceiver, ) = swap.receiver.call{value: receiverAmount}("");
        require(successReceiver, "Receiver transfer failed");

        (bool successSolver, ) = msg.sender.call{value: solverReward}("");
        require(successSolver, "Solver transfer failed");
        emit SolverReward(_swapId, msg.sender, solverReward);
    }

    /**
     * @notice Refund after timelock expires (refund path)
     * @param _swapId Swap identifier
     */
    function refund(bytes32 _swapId) external {
        Swap storage swap = swaps[_swapId];
        require(swap.initialized, "Swap does not exist");
        require(!swap.finalized, "Already finalized");
        require(block.timestamp >= swap.timelock, "Timelock not yet expired");

        swap.finalized = true;

        uint256 solverReward = swap.amount / BPS_DENOMINATOR;
        uint256 receiverAmount = swap.amount - solverReward;

        emit Refund(_swapId);

        (bool success, ) = swap.sender.call{value: receiverAmount}("");
        require(success, "Receiver transfer failed");

        (bool successSolver, ) = msg.sender.call{value: solverReward}("");
        require(successSolver, "Solver transfer failed");
        emit SolverReward(_swapId, msg.sender, solverReward);
    }
}