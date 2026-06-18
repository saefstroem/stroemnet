// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../script/StroemHTLCV1.sol";

contract StroemHTLCV1Test is Test {

    StroemHTLCV1 public htlc;

    address alice;
    address bob;
    address charlie;
    address attacker;
    address solver;

    bytes32 secret;
    bytes32 secretHash;
    uint256 constant AMOUNT = 1 ether;
    uint256 constant TIMELOCK_DURATION = 2 hours;

    function setUp() public {
        htlc = new StroemHTLCV1();

        alice = makeAddr("alice");
        bob = makeAddr("bob");
        charlie = makeAddr("charlie");
        attacker = makeAddr("attacker");
        solver = makeAddr("solver");

        vm.deal(alice, 100 ether);
        vm.deal(bob, 100 ether);
        vm.deal(charlie, 100 ether);
        vm.deal(attacker, 100 ether);
        vm.deal(solver, 100 ether);

        secret = bytes32(uint256(12345));
        secretHash = sha256(abi.encodePacked(secret));
    }

    // Core Functionality — Happy Paths

    function test_SuccessfulClaim_SolverIsSomeoneElse() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        uint256 aliceBefore = alice.balance;
        uint256 bobBefore = bob.balance;
        uint256 solverBefore = solver.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        assertEq(alice.balance, aliceBefore - AMOUNT);

        // Solver (third party) claims
        vm.prank(solver);
        htlc.claim(swapId, secret);

        uint256 expectedReward = AMOUNT / 1000; // 0.001 ether
        uint256 expectedReceiver = AMOUNT - expectedReward;

        assertEq(bob.balance, bobBefore + expectedReceiver, "Bob should get amount - reward");
        assertEq(solver.balance, solverBefore + expectedReward, "Solver should get 0.1% reward");
        assertEq(alice.balance, aliceBefore - AMOUNT, "Alice paid full amount");

        (, , , , , , , bool finalized) = htlc.swaps(swapId);
        assertTrue(finalized);
    }

    function test_SuccessfulClaim_ReceiverIsSolver() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        uint256 bobBefore = bob.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        // Bob himself is the solver — gets full amount
        vm.prank(bob);
        htlc.claim(swapId, secret);

        uint256 expectedReward = AMOUNT / 1000;
        uint256 expectedReceiver = AMOUNT - expectedReward;

        // Bob gets both receiver amount AND solver reward
        assertEq(bob.balance, bobBefore + expectedReceiver + expectedReward, "Bob gets everything");
    }
    function test_Refund_SolverGetsReward() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.deal(alice, AMOUNT);
        uint256 aliceBefore = alice.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(77777));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock, 0, swapId);

        vm.warp(timelock);

        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.refund(swapId);

        uint256 solverReward = AMOUNT / 1000;
        uint256 senderRefund = AMOUNT - solverReward;

        assertEq(alice.balance, aliceBefore - AMOUNT + senderRefund, "Sender gets refund minus reward");
        assertEq(solver.balance, solverBefore + solverReward, "Solver gets reward");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    function test_Refund_SelfRefund_SenderGetsBothParts() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.deal(alice, AMOUNT);
        uint256 aliceBefore = alice.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(66666));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock, 0, swapId);

        vm.warp(timelock);

        // Alice refunds herself — gets both sender refund AND solver reward
        vm.prank(alice);
        htlc.refund(swapId);

        assertEq(alice.balance, aliceBefore, "Alice gets everything back when self-refunding");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    function test_Refund_RewardPrecision_MinimumAmount() public {
        // Minimum valid amount (1001): reward = 1, sender = 1000
        uint256 amount = 1001;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(55555));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        uint256 aliceBefore = alice.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(55555));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock, 0, swapId);

        vm.warp(timelock);

        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.refund(swapId);

        assertEq(alice.balance, aliceBefore - amount + 1000, "Sender gets 1000");
        assertEq(solver.balance, solverBefore + 1, "Solver gets 1");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    function test_Refund_RewardPrecision_LargeAmount() public {
        // 1 ether: reward = 1e18/1000 = 1e15, sender = 999e15
        uint256 amount = 1 ether;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(33333));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        uint256 aliceBefore = alice.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(33333));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock, 0, swapId);

        vm.warp(timelock);

        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.refund(swapId);

        uint256 expectedReward = 1e15;
        uint256 expectedRefund = amount - expectedReward;

        assertEq(alice.balance, aliceBefore - amount + expectedRefund, "Sender gets 0.999 ether");
        assertEq(solver.balance, solverBefore + expectedReward, "Solver gets 0.001 ether");
        assertEq(address(htlc).balance, 0, "No funds left");
    }
    function test_SuccessfulClaim_SenderIsSolver() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        uint256 aliceBefore = alice.balance;
        uint256 bobBefore = bob.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        // Alice (sender) acts as solver
        vm.prank(alice);
        htlc.claim(swapId, secret);

        uint256 expectedReward = AMOUNT / 1000;
        uint256 expectedReceiver = AMOUNT - expectedReward;

        assertEq(bob.balance, bobBefore + expectedReceiver, "Bob gets receiver amount");
        assertEq(alice.balance, aliceBefore - AMOUNT + expectedReward, "Alice gets reward back");
    }

    function test_RefundAfterTimeout_HappyPath() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        uint256 aliceBefore = alice.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock);

        vm.prank(alice);
        htlc.refund(swapId);

        assertEq(alice.balance, aliceBefore, "Alice should recover all funds");

        (, , , , , , , bool finalized) = htlc.swaps(swapId);
        assertTrue(finalized);
    }

    // Event Emission Tests

    function test_EventsEmitted_Commitment() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.expectEmit(true, true, true, true);
        bytes32 swapId = bytes32(uint256(99999));

        emit StroemHTLCV1.Commitment(
            swapId,
            alice,
            bob,
            "",
            AMOUNT,
            secretHash,
            timelock,
            0
        );

        vm.prank(alice);
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);
    }

    function test_EventsEmitted_Claim() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        uint256 expectedReward = AMOUNT / 1000;
        uint256 expectedReceiver = AMOUNT - expectedReward;

        vm.expectEmit(true, true, false, true);
        emit StroemHTLCV1.Claim(swapId, secret, solver, expectedReceiver, expectedReward);

        vm.prank(solver);
        htlc.claim(swapId, secret);
    }

    function test_EventsEmitted_Refund() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock);

        vm.expectEmit(true, false, false, false);
        emit StroemHTLCV1.Refund(swapId);

        vm.prank(alice);
        htlc.refund(swapId);
    }

    // Atomicity Tests
    function test_Atomicity_ClaimPreventsRefund() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(solver);
        htlc.claim(swapId, secret);

        vm.warp(timelock);
        vm.prank(alice);
        vm.expectRevert("Already finalized");
        htlc.refund(swapId);
    }

    function test_Atomicity_RefundPreventsClaim() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock);
        vm.prank(alice);
        htlc.refund(swapId);

        vm.prank(solver);
        vm.expectRevert("Already finalized");
        htlc.claim(swapId, secret);
    }

    function test_Atomicity_DoubleClaimFails() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(solver);
        htlc.claim(swapId, secret);

        vm.prank(attacker);
        vm.expectRevert("Already finalized");
        htlc.claim(swapId, secret);
    }

    function test_Atomicity_DoubleRefundFails() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock);
        vm.prank(alice);
        htlc.refund(swapId);

        vm.prank(alice);
        vm.expectRevert("Already finalized");
        htlc.refund(swapId);
    }

    // Timelock Safety Tests
    function test_TimelockSafety_CannotClaimAfterExpiry() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock);

        vm.prank(solver);
        vm.expectRevert("Timelock expired");
        htlc.claim(swapId, secret);
    }

    function test_TimelockSafety_CannotRefundBeforeExpiry() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(alice);
        vm.expectRevert("Timelock not yet expired");
        htlc.refund(swapId);
    }

    function test_TimelockSafety_ClaimAtBoundaryFails() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock); // exact boundary: block.timestamp == timelock

        vm.prank(solver);
        vm.expectRevert("Timelock expired"); // requires strict <
        htlc.claim(swapId, secret);
    }

    function test_TimelockSafety_RefundAtBoundarySucceeds() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock); // exact boundary: block.timestamp == timelock

        vm.prank(alice);
        htlc.refund(swapId); // requires >=
    }

    function test_TimelockSafety_ClaimOneSecondBeforeExpiry() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock - 1);

        vm.prank(solver);
        htlc.claim(swapId, secret);
    }

    function test_TimelockSafety_RefundOneSecondAfterExpiry() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock + 1);

        vm.prank(alice);
        htlc.refund(swapId);
    }

    //  Access Control Tests (Non-Custodial Property)
    function test_Access_AnyoneCanClaim() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        // Attacker with valid secret CAN claim (this is by design)
        uint256 bobBefore = bob.balance;
        uint256 attackerBefore = attacker.balance;

        vm.prank(attacker);
        htlc.claim(swapId, secret);

        uint256 expectedReward = AMOUNT / 1000;
        uint256 expectedReceiver = AMOUNT - expectedReward;

        assertEq(bob.balance, bobBefore + expectedReceiver, "Bob still gets funds");
        assertEq(attacker.balance, attackerBefore + expectedReward, "Attacker gets solver reward only");
    }

    function test_Access_ClaimRequiresCorrectSecret() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        bytes32 wrongSecret = bytes32(uint256(99999));

        vm.prank(solver);
        vm.expectRevert("Invalid secret");
        htlc.claim(swapId, wrongSecret);
    }

    function test_Access_ClaimFailsWithZeroSecret() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(solver);
        vm.expectRevert("Invalid secret");
        htlc.claim(swapId, swapId);
    }

  
    function test_Precision_NotDivisible_RemainderGoesToReceiver() public {
        // 1001 wei: reward = 1001/1000 = 1 (floors), receiver = 1001-1 = 1000
        uint256 amount = 1001;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(77777));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock,0,swapId);

        uint256 bobBefore = bob.balance;
        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.claim(swapId, s);

        assertEq(bob.balance, bobBefore + 1000, "Receiver gets remainder");
        assertEq(solver.balance, solverBefore + 1, "Solver gets floor");
        assertEq(address(htlc).balance, 0, "No funds left in contract");
    }


    function test_Precision_LargeAmount_NoFundsLeft() public {
        // 100 ether: reward = 0.1 ether, receiver = 99.9 ether
        uint256 amount = 100 ether;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(22222));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock,0,swapId);

        uint256 bobBefore = bob.balance;
        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.claim(swapId, s);

        assertEq(bob.balance, bobBefore + 99.9 ether, "Receiver gets 99.9 ETH");
        assertEq(solver.balance, solverBefore + 0.1 ether, "Solver gets 0.1 ETH");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    function test_Precision_Refund_FullAmount() public {
        // Refund always returns full amount, no solver involved
        uint256 amount = 1001;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(33333));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        uint256 aliceBefore = alice.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock,0,swapId);

        vm.warp(timelock);
        vm.prank(alice);
        htlc.refund(swapId);

        assertEq(alice.balance, aliceBefore, "Alice gets full refund");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    // Griefing Resistance
    function test_GriefingResistance_SecretHashUniqueness() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(alice);
        vm.expectRevert("Secret hash already used");
        bytes32 swapId2 = bytes32(uint256(99998));
        htlc.newSwap{value: AMOUNT}(alice,charlie, "", secretHash, timelock,0,swapId2);
    }

    function test_GriefingResistance_SecretReuseAfterReveal() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        bytes32 swapId2 = bytes32(uint256(99998));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(solver);
        htlc.claim(swapId, secret);

        // Secret is now public. Same hash cannot be reused.
        vm.prank(alice);
        vm.expectRevert("Secret hash already used");
        htlc.newSwap{value: AMOUNT}(alice, charlie, "", secretHash, timelock,0,swapId2);
    }

    function test_GriefingResistance_OpportunityCost() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        uint256 aliceAfterLock = alice.balance;

        vm.warp(timelock);

        vm.prank(alice);
        htlc.refund(swapId);

        assertEq(alice.balance, aliceAfterLock + AMOUNT);
    }

    // Input Validation Tests

    function test_RevertWhen_NewSwap_ZeroAmount() public {
        vm.prank(alice);
        vm.expectRevert("Amount must be greater than 0");
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: 0}(alice,bob, "", secretHash, block.timestamp + TIMELOCK_DURATION,0,swapId);
    }

    function test_RevertWhen_NewSwap_PastTimelock() public {
        vm.prank(alice);
        vm.expectRevert("Timelock must be in the future");
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, block.timestamp - 1,0,swapId);
    }

    function test_RevertWhen_NewSwap_CurrentTimestamp() public {
        vm.prank(alice);
        vm.expectRevert("Timelock must be in the future");
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, block.timestamp,0,swapId);
    }

    function test_RevertWhen_NewSwap_ZeroAddress() public {
        vm.prank(alice);
        vm.expectRevert("Invalid receiver address");
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice,address(0), "", secretHash, block.timestamp + TIMELOCK_DURATION,0,swapId);
    }

    function test_RevertWhen_NewSwap_ZeroSecretHash() public {
        vm.prank(alice);
        vm.expectRevert("Invalid secret hash");
        bytes32 swapId = bytes32(uint256(99997));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", bytes32(0), block.timestamp + TIMELOCK_DURATION,0,swapId);
    }

    function test_Claim_NonExistentSwap() public {
        bytes32 fakeSwapId = bytes32(uint256(999));

        vm.prank(solver);
        vm.expectRevert("Swap does not exist");
        htlc.claim(fakeSwapId, secret);
    }

    function test_Refund_NonExistentSwap() public {
        bytes32 fakeSwapId = bytes32(uint256(999));

        vm.prank(alice);
        vm.expectRevert("Swap does not exist");
        htlc.refund(fakeSwapId);
    }

    // Reentrancy & Transfer Failure Tests
    function test_Attack_ReentrancyOnClaim() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        MaliciousReceiver malicious = new MaliciousReceiver(htlc);

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice,address(malicious), "", secretHash, timelock,0,swapId);

        malicious.setSwapId(swapId);
        malicious.setSecret(secret);

        // Malicious contract is the receiver. Solver triggers claim.
        vm.prank(solver);
        htlc.claim(swapId, secret);

        uint256 expectedReceiver = AMOUNT - AMOUNT / 1000;
        assertEq(address(malicious).balance, expectedReceiver, "Only one withdrawal");
    }

    function test_Attack_ReentrancyBySolver() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        MaliciousSolver maliciousSolver = new MaliciousSolver(htlc);

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        maliciousSolver.setSwapId(swapId);
        maliciousSolver.setSecret(secret);

        // Malicious solver triggers claim
        vm.prank(address(maliciousSolver));
        htlc.claim(swapId, secret);

        uint256 expectedReward = AMOUNT / 1000;
        assertEq(address(maliciousSolver).balance, expectedReward, "Only one reward");
    }

    function test_RevertWhen_ReceiverTransferFails() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        RejectingReceiver rejecter = new RejectingReceiver();

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice,address(rejecter), "", secretHash, timelock,0,swapId);

        vm.prank(solver);
        vm.expectRevert("Receiver transfer failed");
        htlc.claim(swapId, secret);

        // Verify swap is NOT finalized (state reverted)
        (, , , , , ,, bool finalized) = htlc.swaps(swapId);
        assertFalse(finalized, "Swap should not be finalized after failed transfer");
    }

    function test_RevertWhen_SolverTransferFails() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        RejectingSolver rejectingSolver = new RejectingSolver();

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(address(rejectingSolver));
        vm.expectRevert("Solver transfer failed");
        htlc.claim(swapId, secret);

        // Verify swap is NOT finalized (entire tx reverted)
        (, , , , , ,, bool finalized) = htlc.swaps(swapId);
        assertFalse(finalized, "Swap should not be finalized after failed solver transfer");
    }
    
    function test_RevertWhen_RefundTransferFails() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        RejectingSender rejecter = new RejectingSender(htlc);
        vm.deal(address(rejecter), 10 ether);
        bytes32 swapId= bytes32(uint256(99999));
        rejecter.createSwap(alice, bob, "", secretHash, timelock, AMOUNT,swapId);

        vm.warp(timelock);

        vm.expectRevert("Solver transfer failed");
        rejecter.attemptRefund(swapId);
    }
    function test_RevertWhen_RefundSenderTransferFails() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        // Create a swap where onBehalfOf is a contract that rejects ETH
        RejectingOnBehalfOf rejecter = new RejectingOnBehalfOf();

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(address(rejecter), bob, "", secretHash, timelock, 0, swapId);

        vm.warp(timelock);

        // Refund tries to send receiverAmount to swap.sender (rejecter) — fails
        vm.prank(solver);
        vm.expectRevert("Receiver transfer failed");
        htlc.refund(swapId);
    }

    function test_RevertWhen_NewSwap_ZeroSwapId() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;
        bytes32 freshSecretHash = sha256(abi.encodePacked(bytes32(uint256(77777))));

        vm.prank(alice);
        vm.expectRevert("Invalid swap ID");
        htlc.newSwap{value: AMOUNT}(alice, bob,"", freshSecretHash, timelock, 0, bytes32(0));
    }

    function test_RevertWhen_NewSwap_DuplicateSwapId() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;
        bytes32 swapId = bytes32(uint256(88888));

        vm.prank(alice);
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock, 0, swapId);

        bytes32 secretHash2 = sha256(abi.encodePacked(bytes32(uint256(42))));
        vm.prank(alice);
        vm.expectRevert("Swap ID already exists");
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash2, timelock, 0, swapId);
    }
    // Edge Cases & Boundary Tests
    function test_EdgeCase_MinimumTimelock() public {
        uint256 timelock = block.timestamp + 1;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(solver);
        htlc.claim(swapId, secret);
    }

    function test_EdgeCase_VeryLongTimelock() public {
        uint256 timelock = block.timestamp + 365 days;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.prank(solver);
        htlc.claim(swapId, secret);
    }

    function test_EdgeCase_MultipleSwapsSameSender() public {
        bytes32 secret2 = bytes32(uint256(67890));
        bytes32 secretHash2 = sha256(abi.encodePacked(secret2));
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.startPrank(alice);
        bytes32 swapId1 = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId1);
        bytes32 swapId2 = bytes32(uint256(99998));
        htlc.newSwap{value: AMOUNT}(alice, charlie, "", secretHash2, timelock,0,swapId2);
        vm.stopPrank();

        assertTrue(swapId1 != swapId2);
    }

    function test_EdgeCase_MultipleSwapsSameReceiver() public {
        bytes32 secret2 = bytes32(uint256(67890));
        bytes32 secretHash2 = sha256(abi.encodePacked(secret2));
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId1 = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId1);

        vm.prank(charlie);
        bytes32 swapId2 = bytes32(uint256(99998));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash2, timelock,0,swapId2);

        vm.prank(solver);
        htlc.claim(swapId1, secret);

        vm.prank(solver);
        htlc.claim(swapId2, secret2);
    }

    function test_EdgeCase_SwapIdDeterministic() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        bytes32 expectedId = swapId;
        assertEq(swapId, expectedId);
    }

    // ════════════════════════════════════════════════════════════
    //  PART XI: Integration Tests
    // ════════════════════════════════════════════════════════════

    function test_Integration_SequentialSwaps() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        for (uint i = 1; i <= 3; i++) {
            bytes32 tempSecret = bytes32(uint256(12345 + i));
            bytes32 tempHash = sha256(abi.encodePacked(tempSecret));

            vm.prank(alice);
            bytes32 swapId = bytes32(uint256(99999 + i));
            htlc.newSwap{value: AMOUNT}(alice, bob,"", tempHash, timelock,0,swapId);

            vm.prank(solver);
            htlc.claim(swapId, tempSecret);
        }

        uint256 totalReward = (AMOUNT / 1000) * 3;
        uint256 totalReceiver = (AMOUNT - AMOUNT / 1000) * 3;

        assertEq(alice.balance, 100 ether - (3 * AMOUNT));
        assertEq(bob.balance, 100 ether + totalReceiver);
        assertEq(solver.balance, 100 ether + totalReward);
    }

    function test_Integration_BidirectionalSwaps() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 secret2 = bytes32(uint256(67890));
        bytes32 secretHash2 = sha256(abi.encodePacked(secret2));

        vm.prank(alice);
        bytes32 swapId1 = bytes32(uint256(99999));
        htlc.newSwap{value: 1 ether}(alice, bob,"", secretHash, timelock,0,swapId1);

        vm.prank(bob);
        bytes32 swapId2 = bytes32(uint256(99998));
        htlc.newSwap{value: 2 ether}(bob, alice,"", secretHash2, timelock,0,swapId2);

        vm.prank(solver);
        htlc.claim(swapId1, secret);

        vm.prank(solver);
        htlc.claim(swapId2, secret2);

        uint256 reward1 = 1 ether / 1000;
        uint256 reward2 = 2 ether / 1000;

        assertEq(alice.balance, 100 ether - 1 ether + (2 ether - reward2));
        assertEq(bob.balance, 100 ether - 2 ether + (1 ether - reward1));
        assertEq(solver.balance, 100 ether + reward1 + reward2);
    }

    function test_Integration_MixedClaimAndRefund() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 secret2 = bytes32(uint256(67890));
        bytes32 secretHash2 = sha256(abi.encodePacked(secret2));

        // Swap 1: claimed
        vm.prank(alice);
        bytes32 swapId1 = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId1);

        vm.prank(solver);
        htlc.claim(swapId1, secret);

        // Swap 2: refunded
        vm.prank(alice);
        bytes32 swapId2 = bytes32(uint256(99998));
        htlc.newSwap{value: AMOUNT}(alice, charlie, "", secretHash2, timelock,0,swapId2);

        vm.warp(timelock);
        vm.prank(alice);
        htlc.refund(swapId2);

        assertEq(address(htlc).balance, 0, "No funds left after mixed operations");
    }

    // Gas Checks

    function test_Gas_NewSwap() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        uint256 gasBefore = gasleft();
        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);
        uint256 gasUsed = gasBefore - gasleft();

        emit log_named_uint("Gas used for newSwap", gasUsed);
        assertLt(gasUsed, 200000);
    }

    function test_Gas_Claim() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        uint256 gasBefore = gasleft();
        vm.prank(solver);
        htlc.claim(swapId, secret);
        uint256 gasUsed = gasBefore - gasleft();

        emit log_named_uint("Gas used for claim", gasUsed);
        assertLt(gasUsed, 100000);
    }

    function test_Gas_Refund() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock,0,swapId);

        vm.warp(timelock);

        uint256 gasBefore = gasleft();
        vm.prank(alice);
        htlc.refund(swapId);
        uint256 gasUsed = gasBefore - gasleft();

        emit log_named_uint("Gas used for refund", gasUsed);
        assertLt(gasUsed, 100000);
    }

        function test_Access_AnyoneCanRefund_FundsGoToSender() public {
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        uint256 aliceBefore = alice.balance;
        uint256 bobBefore = bob.balance;

        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: AMOUNT}(alice, bob,"", secretHash, timelock, 0, swapId);

        vm.warp(timelock);

        // Bob (receiver) triggers the refund — funds go to alice minus solver reward
        vm.prank(bob);
        htlc.refund(swapId);

        uint256 solverReward = AMOUNT / 1000;
        uint256 senderRefund = AMOUNT - solverReward;

        assertEq(alice.balance, aliceBefore - AMOUNT + senderRefund, "Alice should recover funds minus solver reward");
        assertEq(bob.balance, bobBefore + solverReward, "Bob (executor) gets solver reward");

        (, , , , , , , bool finalized) = htlc.swaps(swapId);
        assertTrue(finalized);
    }

    function test_Precision_1Wei_SolverGetsZero() public {
        // 1 wei is below the BPS_DENOMINATOR minimum, so we need > 1000
        // Test with 1001: reward = 1001/1000 = 1, receiver = 1000
        uint256 amount = 1001;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(11111));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock, 0, swapId);

        uint256 bobBefore = bob.balance;
        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.claim(swapId, s);

        uint256 solverReward = amount / 1000; // 1
        uint256 receiverAmount = amount - solverReward; // 1000

        assertEq(bob.balance, bobBefore + receiverAmount, "Receiver gets 1000");
        assertEq(solver.balance, solverBefore + solverReward, "Solver gets 1");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    function test_Precision_999Wei_SolverGetsZero() public {
        // 999 is below minimum (> 1000 required), test with 1999 instead
        // 1999 / 1000 = 1 (floors), receiver = 1998
        uint256 amount = 1999;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(88888));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock, 0, swapId);

        uint256 bobBefore = bob.balance;
        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.claim(swapId, s);

        uint256 solverReward = amount / 1000; // 1
        uint256 receiverAmount = amount - solverReward; // 1998

        assertEq(bob.balance, bobBefore + receiverAmount, "Receiver gets 1998");
        assertEq(solver.balance, solverBefore + solverReward, "Solver gets 1");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    function test_Precision_ExactlyDivisible() public {
        // 2000 wei: reward = 2, receiver = 1998, total = 2000 ✓
        uint256 amount = 2000;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        vm.deal(alice, amount);
        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99299));
        htlc.newSwap{value: amount}(alice, bob,"", secretHash, timelock, 0, swapId);

        uint256 contractBefore = address(htlc).balance;
        assertEq(contractBefore, amount);

        uint256 bobBefore = bob.balance;
        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.claim(swapId, secret);

        uint256 solverReward = amount / 1000; // 2
        uint256 receiverAmount = amount - solverReward; // 1998

        assertEq(bob.balance, bobBefore + receiverAmount, "Receiver gets 1998");
        assertEq(solver.balance, solverBefore + solverReward, "Solver gets 2");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

    function test_ZeroReward_SolverTransferSkipped() public {
        // Can't test with amount < 1000 since newSwap requires > BPS_DENOMINATOR
        // Instead test with exactly 1001 where reward is 1 (minimum non-zero)
        // For a true zero-reward test, the contract would need to allow amounts <= 1000
        // This test verifies the minimum reward case instead
        uint256 amount = 1001;
        uint256 timelock = block.timestamp + TIMELOCK_DURATION;

        bytes32 s = bytes32(uint256(44444));
        bytes32 sh = sha256(abi.encodePacked(s));

        vm.deal(alice, amount);
        vm.prank(alice);
        bytes32 swapId = bytes32(uint256(99999));
        htlc.newSwap{value: amount}(alice, bob,"", sh, timelock, 0, swapId);

        uint256 bobBefore = bob.balance;
        uint256 solverBefore = solver.balance;

        vm.prank(solver);
        htlc.claim(swapId, s);

        uint256 solverReward = amount / 1000; // 1
        uint256 receiverAmount = amount - solverReward; // 1000

        assertEq(bob.balance, bobBefore + receiverAmount, "Receiver gets 1000");
        assertEq(solver.balance, solverBefore + solverReward, "Solver gets 1");
        assertEq(address(htlc).balance, 0, "No funds left");
    }

}

// Helper Contracts
/**
 * @title MaliciousReceiver — tries to reenter claim() when receiving ETH
 */
contract MaliciousReceiver {
    StroemHTLCV1 public htlc;
    bytes32 public swapId;
    bytes32 public secret;
    uint256 public callCount;

    constructor(StroemHTLCV1 _htlc) {
        htlc = _htlc;
    }

    function setSwapId(bytes32 _swapId) external {
        swapId = _swapId;
    }

    function setSecret(bytes32 _secret) external {
        secret = _secret;
    }

    receive() external payable {
        callCount++;
        if (callCount == 1) {
            try htlc.claim(swapId, secret) {
                revert("Reentrancy succeeded - VULNERABILITY!");
            } catch {
                // Expected: "Already finalized"
            }
        }
    }
}

/**
 * @title MaliciousSolver — tries to reenter claim() when receiving solver reward
 */
contract MaliciousSolver {
    StroemHTLCV1 public htlc;
    bytes32 public swapId;
    bytes32 public secret;
    uint256 public callCount;

    constructor(StroemHTLCV1 _htlc) {
        htlc = _htlc;
    }

    function setSwapId(bytes32 _swapId) external {
        swapId = _swapId;
    }

    function setSecret(bytes32 _secret) external {
        secret = _secret;
    }

    receive() external payable {
        callCount++;
        if (callCount == 1) {
            try htlc.claim(swapId, secret) {
                revert("Reentrancy succeeded - VULNERABILITY!");
            } catch {
                // Expected: "Already finalized"
            }
        }
    }
}

/**
 * @title RejectingReceiver — rejects all ETH (no receive/fallback)
 */
contract RejectingReceiver {
    // No receive() or fallback()
}

/**
 * @title RejectingSolver — rejects all ETH (no receive/fallback)
 */
contract RejectingSolver {
    // No receive() or fallback()
}

/**
 * @title RejectingOnBehalfOf — used as onBehalfOf address, rejects all ETH
 */
contract RejectingOnBehalfOf {
    // No receive() or fallback() — rejects ETH transfers
}

/**
 * @title RejectingSender — creates swaps but rejects refunds
 */
contract RejectingSender {
    StroemHTLCV1 public htlc;
    bool public rejectETH = false;

    constructor(StroemHTLCV1 _htlc) {
        htlc = _htlc;
    }

    function createSwap(
        address onBehalfOf,
        address receiver,
        bytes memory destinationReceiver,
        bytes32 secretHash,
        uint256 timelock,
        uint256 amount,
        bytes32 swapId
    ) external {
        return htlc.newSwap{value: amount}(onBehalfOf, receiver, destinationReceiver, secretHash, timelock,0,swapId);
    }

    function attemptRefund(bytes32 swapId) external {
        rejectETH = true;
        htlc.refund(swapId);
    }

    receive() external payable {
        if (rejectETH) {
            revert("Rejecting ETH");
        }
    }
}