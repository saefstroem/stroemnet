// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "./StroemHTLCV1.sol";

contract DeployScript is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("DEPLOYER_PRIVATE_KEY");
        vm.startBroadcast(deployerPrivateKey);

        StroemHTLCV1 htlc = new StroemHTLCV1();
        console.log("StroemHTLCV1 deployed to:", address(htlc));
        
        vm.stopBroadcast();
    }
}