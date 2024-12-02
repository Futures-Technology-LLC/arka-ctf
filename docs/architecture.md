# CTF Architecture

## **Event Initialization**

During event initialization, two key accounts are created:

1. **Event Account**
   - **Purpose**: Stores metadata related to the event, such as the `event_id` and the event's outcome.
   - **Representation**: This account is implemented as `EventData` in the code.
   - **Future Plans**: In upcoming iterations, we plan to integrate an Oracle service to automatically update the event's outcome during the settlement phase.

2. **Event USDC ATA (Escrow Account)**
   - **Purpose**: Temporarily holds funds associated with the event until the settlement is complete.
   - **Role**: Acts as an escrow to ensure secure and controlled fund transfers.

---

## **User Initialization**

For every user interacting with the platform, two types of on-chain accounts are created:

1. **PDA-Controlled USDC ATA**
   - **Purpose**: Holds user funds securely when the user places a bet on an event.
   - **Functionality**:
     - Prevents double-spending by locking funds until a matching order is found.
     - If no match is found, the user can cancel the order and receive a refund from this account.

2. **User Event Data Account (`UserEventData`)**
   - **Purpose**: Tracks user-specific data for each event.
   - **Details Stored**:
     - **Average Purchase Price**: Reflects the user's average cost per unit for a given event.
     - **Total Quantity**: Indicates the total quantity of assets owned by the user for that event.
   - **Functionality**:
     - Updated whenever the user buys or sells assets for an event.
     - A separate `UserEventData` account is created for each user-event combination.

---

## **Order Initialization**

The platform supports two types of orders:

1. **Buy Order**
   - **Process**:
     - When a user places a bid on an event at a specific price point, funds are transferred from the user's PDA-controlled USDC ATA to the escrow account.
     - If the user is trading on the event for the first time, a new `UserEventData` account is created.
   - **Purpose**: Facilitates secure fund transfers and ensures proper record-keeping for the user.

2. **Sell Order**
   - **Process**:
     - When a user decides to sell their position, they place a sell order that allows another user to take over their position.
     - The associated `UserEventData` account is updated to reflect the transaction.
   - **Purpose**: Provides users with the ability to exit their position while ensuring the integrity of on-chain records.
