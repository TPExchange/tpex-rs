# tpex
This is the core logic of TPEx, a full-feature exchange for completely vanilla Minecraft.
Due to the replicated transaction log architecture of TPEx, this is all you need to write the business logic of a server or a client.

For the actual network server/client code (which you probably are looking for, let's be honest), look in the `tpex-api` crate.

However, even for such users, this code is how you will be inspecting the current state of TPEx.

## Architecture
TPEx works off a transaction log which is saved to disk, formed of JSON objects separated by newlines.
The `apply` function on the `State` object *atomically* adds transactions to this log, either updating the internal state, or doing precisely nothing to it.

All (except one) transactions have an explicit calculation for how they should affect the total amount of items inside of each part of the state.
This can be checked with the `Auditable::soft_audit` method, which asks a component how much it believes it is responsible for.
If you are particularly paranoid, the `Auditable::hard_audit` method will calculate this directly from the internal state from scratch.

Any mismatch between the predicted and audited value of an action will cause the code to immediately panic.
This is intentional: such a panic would directly imply the existance of a logic bug, and that the state would be inconsistent.
