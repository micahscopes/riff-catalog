// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// Flattened from Sourcify exact_match mainnet deployment 0x842979fb52b0c19629ae3186838fccbd06484697
// contract: Staking, verified with solc 0.8.28+commit.7893614a.
// Vendored as a hermetic, self-contained test fixture (no network, no personal path).

// ===== @openzeppelin/contracts/utils/Context.sol =====
// OpenZeppelin Contracts (last updated v5.0.1) (utils/Context.sol)


/**
 * @dev Provides information about the current execution context, including the
 * sender of the transaction and its data. While these are generally available
 * via msg.sender and msg.data, they should not be accessed in such a direct
 * manner, since when dealing with meta-transactions the account sending and
 * paying for execution may not be the actual sender (as far as an application
 * is concerned).
 *
 * This contract is only required for intermediate, library-like contracts.
 */
abstract contract Context {
    function _msgSender() internal view virtual returns (address) {
        return msg.sender;
    }

    function _msgData() internal view virtual returns (bytes calldata) {
        return msg.data;
    }

    function _contextSuffixLength() internal view virtual returns (uint256) {
        return 0;
    }
}

// ===== @openzeppelin/contracts/access/Ownable.sol =====
// OpenZeppelin Contracts (last updated v5.0.0) (access/Ownable.sol)



/**
 * @dev Contract module which provides a basic access control mechanism, where
 * there is an account (an owner) that can be granted exclusive access to
 * specific functions.
 *
 * The initial owner is set to the address provided by the deployer. This can
 * later be changed with {transferOwnership}.
 *
 * This module is used through inheritance. It will make available the modifier
 * `onlyOwner`, which can be applied to your functions to restrict their use to
 * the owner.
 */
abstract contract Ownable is Context {
    address private _owner;

    /**
     * @dev The caller account is not authorized to perform an operation.
     */
    error OwnableUnauthorizedAccount(address account);

    /**
     * @dev The owner is not a valid owner account. (eg. `address(0)`)
     */
    error OwnableInvalidOwner(address owner);

    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    /**
     * @dev Initializes the contract setting the address provided by the deployer as the initial owner.
     */
    constructor(address initialOwner) {
        if (initialOwner == address(0)) {
            revert OwnableInvalidOwner(address(0));
        }
        _transferOwnership(initialOwner);
    }

    /**
     * @dev Throws if called by any account other than the owner.
     */
    modifier onlyOwner() {
        _checkOwner();
        _;
    }

    /**
     * @dev Returns the address of the current owner.
     */
    function owner() public view virtual returns (address) {
        return _owner;
    }

    /**
     * @dev Throws if the sender is not the owner.
     */
    function _checkOwner() internal view virtual {
        if (owner() != _msgSender()) {
            revert OwnableUnauthorizedAccount(_msgSender());
        }
    }

    /**
     * @dev Leaves the contract without owner. It will not be possible to call
     * `onlyOwner` functions. Can only be called by the current owner.
     *
     * NOTE: Renouncing ownership will leave the contract without an owner,
     * thereby disabling any functionality that is only available to the owner.
     */
    function renounceOwnership() public virtual onlyOwner {
        _transferOwnership(address(0));
    }

    /**
     * @dev Transfers ownership of the contract to a new account (`newOwner`).
     * Can only be called by the current owner.
     */
    function transferOwnership(address newOwner) public virtual onlyOwner {
        if (newOwner == address(0)) {
            revert OwnableInvalidOwner(address(0));
        }
        _transferOwnership(newOwner);
    }

    /**
     * @dev Transfers ownership of the contract to a new account (`newOwner`).
     * Internal function without access restriction.
     */
    function _transferOwnership(address newOwner) internal virtual {
        address oldOwner = _owner;
        _owner = newOwner;
        emit OwnershipTransferred(oldOwner, newOwner);
    }
}

// ===== @openzeppelin/contracts/token/ERC20/IERC20.sol =====
// OpenZeppelin Contracts (last updated v5.4.0) (token/ERC20/IERC20.sol)


/**
 * @dev Interface of the ERC-20 standard as defined in the ERC.
 */
interface IERC20 {
    /**
     * @dev Emitted when `value` tokens are moved from one account (`from`) to
     * another (`to`).
     *
     * Note that `value` may be zero.
     */
    event Transfer(address indexed from, address indexed to, uint256 value);

    /**
     * @dev Emitted when the allowance of a `spender` for an `owner` is set by
     * a call to {approve}. `value` is the new allowance.
     */
    event Approval(address indexed owner, address indexed spender, uint256 value);

    /**
     * @dev Returns the value of tokens in existence.
     */
    function totalSupply() external view returns (uint256);

    /**
     * @dev Returns the value of tokens owned by `account`.
     */
    function balanceOf(address account) external view returns (uint256);

    /**
     * @dev Moves a `value` amount of tokens from the caller's account to `to`.
     *
     * Returns a boolean value indicating whether the operation succeeded.
     *
     * Emits a {Transfer} event.
     */
    function transfer(address to, uint256 value) external returns (bool);

    /**
     * @dev Returns the remaining number of tokens that `spender` will be
     * allowed to spend on behalf of `owner` through {transferFrom}. This is
     * zero by default.
     *
     * This value changes when {approve} or {transferFrom} are called.
     */
    function allowance(address owner, address spender) external view returns (uint256);

    /**
     * @dev Sets a `value` amount of tokens as the allowance of `spender` over the
     * caller's tokens.
     *
     * Returns a boolean value indicating whether the operation succeeded.
     *
     * IMPORTANT: Beware that changing an allowance with this method brings the risk
     * that someone may use both the old and the new allowance by unfortunate
     * transaction ordering. One possible solution to mitigate this race
     * condition is to first reduce the spender's allowance to 0 and set the
     * desired value afterwards:
     * https://github.com/ethereum/EIPs/issues/20#issuecomment-263524729
     *
     * Emits an {Approval} event.
     */
    function approve(address spender, uint256 value) external returns (bool);

    /**
     * @dev Moves a `value` amount of tokens from `from` to `to` using the
     * allowance mechanism. `value` is then deducted from the caller's
     * allowance.
     *
     * Returns a boolean value indicating whether the operation succeeded.
     *
     * Emits a {Transfer} event.
     */
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

// ===== @openzeppelin/contracts/utils/ReentrancyGuard.sol =====
// OpenZeppelin Contracts (last updated v5.1.0) (utils/ReentrancyGuard.sol)


/**
 * @dev Contract module that helps prevent reentrant calls to a function.
 *
 * Inheriting from `ReentrancyGuard` will make the {nonReentrant} modifier
 * available, which can be applied to functions to make sure there are no nested
 * (reentrant) calls to them.
 *
 * Note that because there is a single `nonReentrant` guard, functions marked as
 * `nonReentrant` may not call one another. This can be worked around by making
 * those functions `private`, and then adding `external` `nonReentrant` entry
 * points to them.
 *
 * TIP: If EIP-1153 (transient storage) is available on the chain you're deploying at,
 * consider using {ReentrancyGuardTransient} instead.
 *
 * TIP: If you would like to learn more about reentrancy and alternative ways
 * to protect against it, check out our blog post
 * https://blog.openzeppelin.com/reentrancy-after-istanbul/[Reentrancy After Istanbul].
 */
abstract contract ReentrancyGuard {
    // Booleans are more expensive than uint256 or any type that takes up a full
    // word because each write operation emits an extra SLOAD to first read the
    // slot's contents, replace the bits taken up by the boolean, and then write
    // back. This is the compiler's defense against contract upgrades and
    // pointer aliasing, and it cannot be disabled.

    // The values being non-zero value makes deployment a bit more expensive,
    // but in exchange the refund on every call to nonReentrant will be lower in
    // amount. Since refunds are capped to a percentage of the total
    // transaction's gas, it is best to keep them low in cases like this one, to
    // increase the likelihood of the full refund coming into effect.
    uint256 private constant NOT_ENTERED = 1;
    uint256 private constant ENTERED = 2;

    uint256 private _status;

    /**
     * @dev Unauthorized reentrant call.
     */
    error ReentrancyGuardReentrantCall();

    constructor() {
        _status = NOT_ENTERED;
    }

    /**
     * @dev Prevents a contract from calling itself, directly or indirectly.
     * Calling a `nonReentrant` function from another `nonReentrant`
     * function is not supported. It is possible to prevent this from happening
     * by making the `nonReentrant` function external, and making it call a
     * `private` function that does the actual work.
     */
    modifier nonReentrant() {
        _nonReentrantBefore();
        _;
        _nonReentrantAfter();
    }

    function _nonReentrantBefore() private {
        // On the first call to nonReentrant, _status will be NOT_ENTERED
        if (_status == ENTERED) {
            revert ReentrancyGuardReentrantCall();
        }

        // Any calls to nonReentrant after this point will fail
        _status = ENTERED;
    }

    function _nonReentrantAfter() private {
        // By storing the original value once again, a refund is triggered (see
        // https://eips.ethereum.org/EIPS/eip-2200)
        _status = NOT_ENTERED;
    }

    /**
     * @dev Returns true if the reentrancy guard is currently set to "entered", which indicates there is a
     * `nonReentrant` function in the call stack.
     */
    function _reentrancyGuardEntered() internal view returns (bool) {
        return _status == ENTERED;
    }
}

// ===== contracts/Staking.sol =====
interface IGwei {
    function burn(uint256 amount) external;
}

/**
 * @title Staking
 * @notice Pure revenue-sharing yield staking for GWEI token
 */
contract Staking is Ownable, ReentrancyGuard {
    // ============ Constants ============

    uint256 public constant PRECISION = 1e18;
    uint256 public constant MAX_COMPOUND_FEE = 0.005 ether;

    // ============ External Contracts ============

    IERC20 public immutable gweiToken;
    address public treasury;

    // ============ Global State ============

    uint256 public totalStaked;
    uint256 public accYieldPerShare;           // Accumulated yield per staked token (scaled by PRECISION)
    uint256 public totalYieldDistributed;

    // ============ Config ============

    uint256 public minStake = 1e18 / 10;            // 0.1 GWEI minimum
    uint256 public compoundCooldown = 1 days;
    uint256 public compoundFee = 0.001 ether;   // 0.001 ETH per compound

    // ============ Per-User State ============

    struct StakeInfo {
        uint256 balance;              // Staked GWEI amount
        uint256 pendingRewards;       // Accumulated unclaimed yield
        uint256 compoundFeeReserve;   // ETH deposited for auto-compound fees
        uint64 lastClaimAt;
        uint64 lastDepositAt;
        uint64 lastWithdrawAt;
    }

    mapping(address => StakeInfo) public stakes;
    mapping(address => uint256) public userRewardsDebt;  // Separate mapping for precision

    // ============ Events ============

    event Deposited(address indexed user, uint256 amount, uint256 newBalance);
    event Withdrawn(address indexed user, uint256 amount, uint256 newBalance);
    event YieldClaimed(address indexed user, uint256 amount);
    event YieldCompounded(address indexed user, uint256 amount, address indexed compounder, uint256 fee);
    event YieldDistributed(uint256 amount, uint256 newAccYieldPerShare);
    event YieldBurned(uint256 amount);
    event CompoundFeeDeposited(address indexed user, uint256 amount);
    event CompoundFeeRefunded(address indexed user, uint256 amount);
    event ConfigUpdated(string param, uint256 oldValue, uint256 newValue);

    // ============ Errors ============

    error ZeroAddress();
    error ZeroAmount();
    error InsufficientBalance();
    error InsufficientPendingRewards();
    error InsufficientCompoundFeeReserve();
    error BelowMinimumStake();
    error CompoundCooldownNotMet();
    error OnlyTreasury();
    error TransferFailed();
    error InvalidConfig();

    // ============ Modifiers ============

    modifier onlyTreasury() {
        if (msg.sender != treasury) revert OnlyTreasury();
        _;
    }

    // ============ Constructor ============

    constructor(address _gweiToken, address _treasury) Ownable(msg.sender) {
        if (_gweiToken == address(0)) revert ZeroAddress();
        if (_treasury == address(0)) revert ZeroAddress();

        gweiToken = IERC20(_gweiToken);
        treasury = _treasury;
    }

    // ============ Core Staking Functions ============

    /**
     * @notice Stake GWEI tokens, optionally deposit ETH for auto-compound fee reserve
     * @param amount Amount of GWEI to stake
     * @dev Send ETH with the call to also fund compound fee reserve
     */
    function deposit(uint256 amount) external payable nonReentrant {
        if (amount == 0) revert ZeroAmount();

        StakeInfo storage stake = stakes[msg.sender];

        // Update rewards before modifying balance (also sets userRewardsDebt)
        _updateRewards(msg.sender);

        uint256 newBalance = stake.balance + amount;
        if (newBalance < minStake) revert BelowMinimumStake();

        // Transfer GWEI from user
        if (!gweiToken.transferFrom(msg.sender, address(this), amount)) revert TransferFailed();

        // Update state
        stake.balance = newBalance;
        stake.lastDepositAt = uint64(block.timestamp);
        // Initialize lastClaimAt for new stakers (starts 24h cooldown for autocompounds)
        if (stake.lastClaimAt == 0) {
            stake.lastClaimAt = uint64(block.timestamp);
        }
        totalStaked += amount;

        // Sync debt after balance change
        // For existing stakers, _updateRewards already set this value
        // For new stakers (balance was 0), _updateRewards returned early without setting debt
        userRewardsDebt[msg.sender] = accYieldPerShare;

        // Add any ETH sent as compound fee reserve
        if (msg.value > 0) {
            stake.compoundFeeReserve += msg.value;
            emit CompoundFeeDeposited(msg.sender, msg.value);
        }

        emit Deposited(msg.sender, amount, newBalance);
    }

    /**
     * @notice Withdraw staked GWEI tokens
     * @param amount Amount of GWEI to withdraw
     */
    function withdraw(uint256 amount) external nonReentrant {
        if (amount == 0) revert ZeroAmount();

        StakeInfo storage stake = stakes[msg.sender];
        if (stake.balance < amount) revert InsufficientBalance();

        // Update rewards before modifying balance (also sets userRewardsDebt)
        _updateRewards(msg.sender);

        uint256 newBalance = stake.balance - amount;

        // If withdrawing all, auto-refund compound fee reserve
        uint256 feeRefund = 0;
        if (newBalance == 0) {
            feeRefund = stake.compoundFeeReserve;
            stake.compoundFeeReserve = 0;
        }

        // Update state
        stake.balance = newBalance;
        stake.lastWithdrawAt = uint64(block.timestamp);
        totalStaked -= amount;

        // Transfer GWEI to user
        if (!gweiToken.transfer(msg.sender, amount)) revert TransferFailed();

        // Refund compound fee reserve if applicable
        if (feeRefund > 0) {
            _safeTransferETH(msg.sender, feeRefund);
            emit CompoundFeeRefunded(msg.sender, feeRefund);
        }

        emit Withdrawn(msg.sender, amount, newBalance);
    }

    // ============ Claim Functions ============

    /**
     * @notice Claim all pending yield rewards
     */
    function claimYield() external nonReentrant {
        _updateRewards(msg.sender);

        StakeInfo storage stake = stakes[msg.sender];
        uint256 pending = stake.pendingRewards;
        if (pending == 0) revert InsufficientPendingRewards();

        // Safety net: cap at available yield (contract balance minus staked)
        uint256 availableYield = gweiToken.balanceOf(address(this)) - totalStaked;
        if (pending > availableYield) {
            // Only deduct actually-paid amount, keep remainder for later
            stake.pendingRewards = pending - availableYield;
            pending = availableYield;
        } else {
            stake.pendingRewards = 0;
        }
        stake.lastClaimAt = uint64(block.timestamp);

        if (!gweiToken.transfer(msg.sender, pending)) revert TransferFailed();

        emit YieldClaimed(msg.sender, pending);
    }

    /**
     * @notice Claim partial yield rewards
     * @param amount Amount of yield to claim
     */
    function claimYieldPartial(uint256 amount) external nonReentrant {
        if (amount == 0) revert ZeroAmount();

        _updateRewards(msg.sender);

        StakeInfo storage stake = stakes[msg.sender];
        if (amount > stake.pendingRewards) revert InsufficientPendingRewards();

        // Safety net
        uint256 availableYield = gweiToken.balanceOf(address(this)) - totalStaked;
        if (amount > availableYield) {
            amount = availableYield;
        }

        stake.pendingRewards -= amount;
        stake.lastClaimAt = uint64(block.timestamp);

        if (!gweiToken.transfer(msg.sender, amount)) revert TransferFailed();

        emit YieldClaimed(msg.sender, amount);
    }

    // ============ Compound Functions ============

    /**
     * @notice Compound own pending rewards
     */
    function compound() external nonReentrant {
        _compound(msg.sender, msg.sender, false);
    }

    /**
     * @notice Compound rewards for another user
     * @dev Requires cooldown elapsed and sufficient fee reserve
     * @param user Address to compound for
     */
    function compoundFor(address user) external nonReentrant {
        if (user == address(0)) revert ZeroAddress();

        StakeInfo storage stake = stakes[user];

        // Verify cooldown
        if (block.timestamp < stake.lastClaimAt + compoundCooldown) {
            revert CompoundCooldownNotMet();
        }

        // Verify fee reserve
        if (stake.compoundFeeReserve < compoundFee) {
            revert InsufficientCompoundFeeReserve();
        }

        // Deduct fee from reserve
        stake.compoundFeeReserve -= compoundFee;

        // Execute compound
        _compound(user, msg.sender, true);

        // Pay compounder
        _safeTransferETH(msg.sender, compoundFee);
    }

    /**
     * @notice Internal compound logic
     * @param user User whose rewards to compound
     * @param compounder Address executing the compound
     * @param payFee Whether to charge fee
     */
    function _compound(address user, address compounder, bool payFee) internal {
        _updateRewards(user);

        StakeInfo storage stake = stakes[user];
        uint256 pending = stake.pendingRewards;
        if (pending == 0) revert InsufficientPendingRewards();

        // Safety net
        uint256 availableYield = gweiToken.balanceOf(address(this)) - totalStaked;
        if (pending > availableYield) {
            // Only deduct actually-paid amount, keep remainder for later
            stake.pendingRewards = pending - availableYield;
            pending = availableYield;
        } else {
            stake.pendingRewards = 0;
        }

        // Update timestamp
        stake.lastClaimAt = uint64(block.timestamp);

        // Add to stake
        stake.balance += pending;
        totalStaked += pending;

        // Update debt for new balance
        userRewardsDebt[user] = accYieldPerShare;

        uint256 fee = payFee ? compoundFee : 0;

        emit YieldCompounded(user, pending, compounder, fee);
    }

    // ============ Compound Fee Reserve Functions ============

    /**
     * @notice Deposit ETH to enable auto-compounding
     */
    function depositCompoundFee() external payable nonReentrant {
        if (msg.value == 0) revert ZeroAmount();

        stakes[msg.sender].compoundFeeReserve += msg.value;

        emit CompoundFeeDeposited(msg.sender, msg.value);
    }

    /**
     * @notice Withdraw unused compound fee reserve
     * @param amount Amount of ETH to withdraw
     */
    function withdrawCompoundFee(uint256 amount) external nonReentrant {
        if (amount == 0) revert ZeroAmount();

        StakeInfo storage stake = stakes[msg.sender];
        if (amount > stake.compoundFeeReserve) revert InsufficientCompoundFeeReserve();

        stake.compoundFeeReserve -= amount;

        _safeTransferETH(msg.sender, amount);

        emit CompoundFeeRefunded(msg.sender, amount);
    }

    // ============ Yield Distribution Functions ============

    /**
     * @notice Distribute yield from Treasury (called after buyback)
     * @dev Only callable by Treasury contract
     * @param amount Amount of GWEI to distribute (already transferred to this contract)
     */
    function distributeYield(uint256 amount) external onlyTreasury {
        _distributeYield(amount);
    }

    /**
     * @notice Manually add GWEI to stakers
     * @param amount Amount of GWEI to add
     */
    function giveGwei(uint256 amount) external nonReentrant {
        if (amount == 0) revert ZeroAmount();

        // Transfer GWEI from caller
        if (!gweiToken.transferFrom(msg.sender, address(this), amount)) revert TransferFailed();

        _distributeYield(amount);
    }

    /**
     * @notice Internal yield distribution logic
     * @param amount Amount to distribute
     */
    function _distributeYield(uint256 amount) internal {
        if (totalStaked == 0) {
            // No stakers. Burn the GWEI to prevent loss
            IGwei(address(gweiToken)).burn(amount);
            emit YieldBurned(amount);
            return;
        }

        accYieldPerShare += (amount * PRECISION) / totalStaked;
        totalYieldDistributed += amount;

        emit YieldDistributed(amount, accYieldPerShare);
    }

    // ============ Core Math ============

    /**
     * @notice Update user's pending rewards based on accumulator
     * @dev Must be called BEFORE modifying user's balance
     * @param user Address to update
     */
    function _updateRewards(address user) internal {
        StakeInfo storage stake = stakes[user];

        if (stake.balance == 0) return;

        uint256 debt = userRewardsDebt[user];
        if (accYieldPerShare > debt) {
            uint256 accumulated = accYieldPerShare - debt;
            uint256 personalRewards = (accumulated * stake.balance) / PRECISION;
            stake.pendingRewards += personalRewards;
        }

        userRewardsDebt[user] = accYieldPerShare;
    }

    // ============ Admin Functions ============

    /**
     * @notice Update treasury address
     * @param _treasury New treasury address
     */
    function setTreasury(address _treasury) external onlyOwner {
        if (_treasury == address(0)) revert ZeroAddress();
        treasury = _treasury;
    }

    /**
     * @notice Update minimum stake amount
     * @param _minStake New minimum
     */
    function setMinStake(uint256 _minStake) external onlyOwner {
        uint256 old = minStake;
        minStake = _minStake;
        emit ConfigUpdated("minStake", old, _minStake);
    }

    /**
     * @notice Update compound cooldown period
     * @param _cooldown New cooldown in seconds
     */
    function setCompoundCooldown(uint256 _cooldown) external onlyOwner {
        uint256 old = compoundCooldown;
        compoundCooldown = _cooldown;
        emit ConfigUpdated("compoundCooldown", old, _cooldown);
    }

    /**
     * @notice Update compound fee (flat ETH per compound)
     * @param _fee New fee in wei
     */
    function setCompoundFee(uint256 _fee) external onlyOwner {
        if (_fee > MAX_COMPOUND_FEE) revert InvalidConfig();
        uint256 old = compoundFee;
        compoundFee = _fee;
        emit ConfigUpdated("compoundFee", old, _fee);
    }

    // ============ View Functions ============

    /**
     * @notice Get pending rewards for a user (includes uncalculated accumulator delta)
     * @param user Address to check
     * @return pending Total pending rewards
     */
    function getPendingRewards(address user) external view returns (uint256 pending) {
        StakeInfo storage stake = stakes[user];
        pending = stake.pendingRewards;

        if (stake.balance > 0) {
            uint256 debt = userRewardsDebt[user];
            if (accYieldPerShare > debt) {
                uint256 accumulated = accYieldPerShare - debt;
                pending += (accumulated * stake.balance) / PRECISION;
            }
        }
    }

    /**
     * @notice Get full stake info for a user
     * @param user Address to check
     */
    function getStakeInfo(address user) external view returns (
        uint256 balance,
        uint256 pendingRewards,
        uint256 compoundFeeReserve,
        uint64 lastClaimAt,
        uint64 lastDepositAt,
        uint64 lastWithdrawAt,
        bool canCompound
    ) {
        StakeInfo storage stake = stakes[user];
        balance = stake.balance;
        pendingRewards = this.getPendingRewards(user);
        compoundFeeReserve = stake.compoundFeeReserve;
        lastClaimAt = stake.lastClaimAt;
        lastDepositAt = stake.lastDepositAt;
        lastWithdrawAt = stake.lastWithdrawAt;
        canCompound = block.timestamp >= lastClaimAt + compoundCooldown && pendingRewards > 0;
    }

    /**
     * @notice Check if someone can compound for a user
     * @param user Address to check
     * @return canDo Whether compound is possible
     * @return reason Reason if cannot compound
     */
    function canCompoundFor(address user) external view returns (bool canDo, string memory reason) {
        StakeInfo storage stake = stakes[user];

        if (stake.balance == 0) return (false, "No stake");
        if (this.getPendingRewards(user) == 0) return (false, "No pending rewards");
        if (block.timestamp < stake.lastClaimAt + compoundCooldown) return (false, "Cooldown not met");
        if (stake.compoundFeeReserve < compoundFee) return (false, "Insufficient fee reserve");

        return (true, "");
    }

    /**
     * @notice Get global staking statistics
     */
    function getGlobalStats() external view returns (
        uint256 _totalStaked,
        uint256 _totalYieldDistributed,
        uint256 _accYieldPerShare
    ) {
        return (totalStaked, totalYieldDistributed, accYieldPerShare);
    }

    // ============ Internal Helpers ============

    /**
     * @notice Safe ETH transfer with success check
     * @param to Recipient address
     * @param amount Amount to transfer
     */
    function _safeTransferETH(address to, uint256 amount) internal {
        (bool success, ) = to.call{value: amount}("");
        if (!success) revert TransferFailed();
    }

    // ============ Receive ============

    receive() external payable {
        // Only accept ETH through depositCompoundFee()
        revert("Use depositCompoundFee()");
    }
}
