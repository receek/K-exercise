# K-exercise

A simple bank transaction processor. It reads a CSV file of client
transactions, applies them in order to per-client accounts, and writes the
resulting account balances back out as CSV.

This project was developed and tested with `rustc 1.97.1`.

## Usage

```sh
cargo run --release -- input.csv > accounts.csv
```

**Be carefull, program overwrites the input file!**

The program takes exactly one command-line argument: the path to the input
CSV file. If it is missing, if extra arguments are given, or if the file does
not exist, an explanatory message is printed to stderr and the program exits
with a non-zero status. The resulting account snapshot is written to stdout.

Any transaction that fails to parse or is rejected while being processed is
reported on stderr and skipped — it does not stop the rest of the file from
being processed.

## Input file format

The input is a CSV file with a header row and four columns:

| column   | type                | description                                                             |
| -------- | ------------------- | ------------------------------------------------------------------------ |
| `type`   | string               | the transaction type: `deposit`, `withdrawal`, `dispute`, `resolve` or `chargeback` (lowercase only) |
| `client` | integer (`u16`)      | the client the transaction belongs to                                    |
| `tx`     | integer (`u32`)      | the transaction's identifier                                             |
| `amount` | decimal string       | the transaction amount (see below); ignored for `dispute`/`resolve`/`chargeback` |

Whitespace around the header names and around field values is trimmed, so
both compact and loosely-formatted CSV files are accepted.

`tx` is assumed to be a globally unique key: every `deposit` and `withdrawal`
introduces a new `tx` that is never reused by that client or by any other
client for the rest of the file.

`amount` is required for `deposit` and `withdrawal` transactions. It must be
a strictly positive decimal number with at most 4 digits after the decimal
point (trailing zeros count towards that limit, e.g. `1.50000` is rejected).
It is not read at all for `dispute`, `resolve` and `chargeback` transactions,
so its value (including being empty) is irrelevant for those rows.

## Output format

The program writes one CSV row per client to stdout, with the columns
`client`, `available`, `held`, `total` and `locked`:

| column      | description                                              |
| ----------- | --------------------------------------------------------- |
| `client`    | the client identifier                                     |
| `available` | funds available for withdrawal or further transactions    |
| `held`      | funds currently held due to an open dispute                |
| `total`     | the sum of `available` and `held`                          |
| `locked`    | `true` or `false` — whether the account has been frozen     |

Row order is not guaranteed.

## Account fields

Each client account tracks:

* `client_id` — the `u16` identifier of the client.
* `available` — the funds the client can freely withdraw or spend.
* `held` — funds temporarily set aside because of an open dispute.
* `locked` — whether the account is frozen; `false` by default, and set to
  `true` permanently once a `chargeback` is processed.

`available` can become negative. This happens when a `deposit` is disputed
after some of its funds have already been withdrawn — the disputed amount is
still moved out of `available` in full, which can drive it below zero. This
is expected behavior of reversing a transaction, not an error condition.

All transactions are isolated between clients: a transaction always affects
only the client it names. There is no operation that moves funds from one
client to another (e.g. no "client 1 sends funds to client 2"), and a
`dispute`, `resolve` or `chargeback` can only ever refer to a `tx` that the
same client originally executed.

## Transaction types

### `Deposit { client, tx, amount }`

Adds `amount` to the client's `available` funds.

* Rejected if the account is locked.
* Rejected if `tx` has already been used by this client (for a deposit,
  withdrawal, or a still-open or already-settled dispute). `Deposit` and
  `Withdrawal` keys are assumed to be unique globally, so this per-client
  check is sufficient in practice.

### `Withdrawal { client, tx, amount }`

Subtracts `amount` from the client's `available` funds.

* Rejected if `amount` is greater than the current `available` balance — a
  client can never withdraw more funds than they have available.
* Rejected if the account is locked.
* Rejected if `tx` has already been used by this client, same as `Deposit`
  — again relying on `tx` being unique across the whole file, not just per
  client.

### `Dispute { client, tx }`

Opens a dispute against a transaction the client previously executed,
referenced by `tx`. It is rejected (as an invalid transaction) if `tx` does
not refer to one of this client's own deposits or withdrawals, or if it has
already been disputed — **a transaction can be disputed at most once**.

A dispute is a valid operation even while the account is locked.

Its effect depends on the disputed transaction's type:

* Disputing a `Deposit` moves `amount` from `available` to `held` (this is
  where `available` can go negative, as described above).
* Disputing a `Withdrawal` adds `amount` to `held` without touching
  `available` — the funds already left `available` when the withdrawal was
  processed, so only the amount now considered "at risk" is set aside.

### `Resolve { client, tx }`

Resolves an open dispute referenced by `tx`, acting as a rejection of that
dispute: it undoes the effect that opening the dispute had, releasing the
held funds back to normal. It is rejected if `tx` has no matching open
dispute for this client.

A resolve is a valid operation even while the account is locked.

* Resolving a disputed `Deposit` moves `amount` back from `held` to
  `available`.
* Resolving a disputed `Withdrawal` removes `amount` from `held` (the funds
  remain withdrawn, exactly as before the dispute was opened).

### `Chargeback { client, tx }`

Reverses the disputed transaction referenced by `tx` for good, and locks the
client's account. It is rejected if `tx` has no matching open dispute for
this client.

A chargeback is a valid operation even while the account is already locked.

* Charging back a `Deposit` removes `amount` from `held`; the funds are not
  returned to `available` — the deposit is permanently reversed.
* Charging back a `Withdrawal` removes `amount` from `held` and returns it to
  `available`, undoing the withdrawal.

Once a chargeback succeeds, the account is locked, which blocks any further
`Deposit` or `Withdrawal` transactions for that client.
