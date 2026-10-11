# OrdiFi UTXO Machine

A small contract interpreter for Bitcoin UTXO state machines.

The Machine does not hardcode business models such as vending, pawn,
auction or escrow. Contracts define the rules.

The runtime reduces those rules to four UTXO operations:

- compose
- split
- insert
- extract

Bitcoin Composition Engine (BCE) performs the physical Bitcoin UTXO operation.

## Architecture

Agent / Client
-> UTXO Contract
-> ordifi-machine
-> Execution Plan
-> BCE
-> Bitcoin Core + ord

The contract defines semantics.

The Machine runtime resolves contract references, runtime actor values,
conditions, effects and required signatures.

BCE remains responsible for Bitcoin transaction geometry and physical
UTXO execution.

## Contract Protocol

Contracts use protocol:

- protocol: ordifi-utxo-contract
- version: 1
- type: vending, pawn, auction, escrow or another contract type

Each action selects one of the four UTXO operations.

## Runtime Context

Contracts may use runtime references such as:

- actor.paymentAddress
- actor.ordinalsAddress
- buyer.ordinalsAddress
- collateral.inscriptionId
- contract

The special value `contract` resolves to `context.contractId`.

## Conditions

Currently implemented:

- block_height_min
- block_height_max
- payment
- collateral_owner

Block-height conditions are evaluated locally.

Payment conditions are emitted as transaction requirements.

collateral_owner currently emits a requires_chain_check requirement.

## BCE Integration

The runtime can currently compile extract execution plans into BCE
group and destination data.

Offline mode:

    ordifi-machine CONTRACT ACTION --context context.json --bce-state current-state.json

Live Machine UTXO mode:

    ordifi-machine CONTRACT ACTION --context context.json --machine-outpoint TXID:VOUT --bce-bin /path/to/bce

The live path currently performs:

BCE inspect-current-state
-> Machine execution plan
-> BCE extract-plan

The BCE binary can also be configured with:

    ORDIFI_MACHINE_BCE_BIN

If no binary is specified, the runtime currently defaults to:

    bce-neutral-node

### Current limitation

Direct BCE integration currently supports extract only.

The contract interpreter understands compose, split, insert and extract,
but live BCE bridges for compose, split and insert are still to be added.

## Vending

utxocontracts/utxocontract.json_vending is the current Vending v1 template.

A purchase expresses:

Asset -> buyer Ordinals address
BTC -> sale payment address
Contract -> contract archive

The seller does not need to sign the final purchase transaction.

Placing the asset and contract into the Machine is the seller-side
authorization step.

The buyer signs the purchase action.

## Pawn

The existing Pawn JSON is an early prototype.

It currently uses fixed borrower information, fixed collateral and
absolute block heights. It is not the final Pawn contract model.

The planned Pawn model binds borrower, collateral and activation
blockheight when collateral is inserted into the Machine.

The Bitcoin L1 activation transaction is intended to become the
authoritative origin of that deal state.

Do not treat the current Pawn example as a finalized protocol.

## Build

    cargo build --release

## CLI

    ordifi-machine <contract.json> <action>       [--block-height N]       [--context context.json]       [--bce-state state.json]       [--machine-outpoint TXID:VOUT]       [--bce-bin PATH]

## Status

0.1.0 is an experimental public baseline.

Contracts define rules.
The Machine interprets them.
BCE changes Bitcoin UTXO state.
