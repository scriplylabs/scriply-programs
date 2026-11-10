// The instructions each action sends, with every account the program
// checks named here rather than left to a resolver, so what is signed is
// readable in one place.

import {
  AccountRole,
  address,
  getAddressEncoder,
  getProgramDerivedAddress,
  type Address,
  type Instruction,
  type TransactionSigner,
} from '@solana/kit';
import { getCreateAssociatedTokenIdempotentInstructionAsync, TOKEN_PROGRAM_ADDRESS } from '@solana-program/token';

import { findGenerationRecord, findTokenAccount, isWithinOneTransaction, TOKEN_2022_PROGRAM_ADDRESS, type TokenProof } from './chain';
import {
  findLoanPda,
  findPoolAuthorityPda,
  getBorrowInstructionAsync,
  getClaimWithdrawalInstructionAsync,
  getDepositInstructionAsync,
  getMarkLoansInstruction,
  getMarkStaleInstruction,
  getRepayInstructionAsync,
  getRequestWithdrawalInstructionAsync,
  getSettleEpochInstructionAsync,
  getSyncLoanInstructionAsync,
  getWithdrawUnitsInstructionAsync,
  type Loan,
  type Pool,
} from './generated/receivables-pool';

/** The Solana Attestation Service, the same on mainnet and devnet. */
const SAS_PROGRAM_ADDRESS = address('22zoJMtdu4tQc2PzL74ZUT7FrwgB1Udec8DdW4yw4BdG');

/** A pool and where it is. */
export type PoolAt = { address: Address; pool: Pool };

/**
 * Moves the pool's epoch settlement on as far as it can go; it does nothing
 * otherwise, so it goes first in every transaction that needs a settled
 * epoch (README: `settle_epoch`).
 */
export async function buildSettleEpoch(payer: TransactionSigner, p: PoolAt): Promise<Instruction> {
  return getSettleEpochInstructionAsync({
    payer,
    pool: p.address,
    shareMint: p.pool.shareMint,
    generationRecord: await findGenerationRecord(p.address, p.pool.withdrawalGeneration),
  });
}

/** The depositor's SAS attestation, at the address SAS derives from the wallet. */
async function findAttestation(p: Pool, wallet: Address): Promise<Address> {
  const encoder = getAddressEncoder();
  const [attestation] = await getProgramDerivedAddress({
    programAddress: SAS_PROGRAM_ADDRESS,
    seeds: ['attestation', encoder.encode(p.attestationCredential), encoder.encode(p.attestationSchema), encoder.encode(wallet)],
  });
  return attestation;
}

export async function buildDeposit(depositor: TransactionSigner, p: PoolAt, amount: bigint): Promise<Instruction[]> {
  return [
    await buildSettleEpoch(depositor, p),
    await getDepositInstructionAsync({
      depositor,
      pool: p.address,
      quoteMint: p.pool.quoteMint,
      shareMint: p.pool.shareMint,
      depositorQuote: await findTokenAccount(depositor.address, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS),
      attestation: p.pool.requiresAttestation === 1 ? await findAttestation(p.pool, depositor.address) : undefined,
      quoteTokenProgram: TOKEN_PROGRAM_ADDRESS,
      amount,
    }),
  ];
}

export async function buildRequestWithdrawal(owner: TransactionSigner, p: PoolAt, shares: bigint): Promise<Instruction[]> {
  return [
    await buildSettleEpoch(owner, p),
    await getRequestWithdrawalInstructionAsync({
      owner,
      pool: p.address,
      shareMint: p.pool.shareMint,
      ownerShares: await findTokenAccount(owner.address, p.pool.shareMint, TOKEN_PROGRAM_ADDRESS),
      shares,
    }),
  ];
}

export async function buildClaimWithdrawal(owner: TransactionSigner, p: PoolAt, generation: bigint): Promise<Instruction[]> {
  const ownerQuote = await findTokenAccount(owner.address, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS);
  return [
    await getCreateAssociatedTokenIdempotentInstructionAsync({ payer: owner, owner: owner.address, mint: p.pool.quoteMint, tokenProgram: TOKEN_PROGRAM_ADDRESS }),
    await getClaimWithdrawalInstructionAsync({
      owner,
      pool: p.address,
      quoteMint: p.pool.quoteMint,
      ownerQuote,
      generationRecord: await findGenerationRecord(p.address, generation),
      quoteTokenProgram: TOKEN_PROGRAM_ADDRESS,
    }),
  ];
}

/** The loan's address and vault for a borrower's loan against `mint` in a pool. */
export async function findLoanVault(poolAddress: Address, mint: Address, borrower: Address): Promise<{ loan: Address; vault: Address }> {
  const [loan] = await findLoanPda({ pool: poolAddress, collateralMint: mint, borrower });
  return { loan, vault: await findTokenAccount(loan, mint, TOKEN_2022_PROGRAM_ADDRESS) };
}

/**
 * Creates the loan's vault, whose creator pays its rent, and the borrower's
 * USDC account if it is missing. The vault starts frozen; the token's issuer
 * thaws it by rule before the first loan (README).
 */
export async function buildCreateVault(borrower: TransactionSigner, p: PoolAt, mint: Address): Promise<Instruction[]> {
  const { loan } = await findLoanVault(p.address, mint, borrower.address);
  return [
    await getCreateAssociatedTokenIdempotentInstructionAsync({ payer: borrower, owner: loan, mint, tokenProgram: TOKEN_2022_PROGRAM_ADDRESS }),
    await getCreateAssociatedTokenIdempotentInstructionAsync({ payer: borrower, owner: borrower.address, mint: p.pool.quoteMint, tokenProgram: TOKEN_PROGRAM_ADDRESS }),
  ];
}

export type BorrowRequest = { mint: Address; units: bigint; principal: bigint; maturityEpochs: number; proof: TokenProof };

export async function buildBorrow(borrower: TransactionSigner, p: PoolAt, r: BorrowRequest): Promise<Instruction[]> {
  const { loan, vault } = await findLoanVault(p.address, r.mint, borrower.address);
  return [
    await buildSettleEpoch(borrower, p),
    await getBorrowInstructionAsync({
      borrower,
      pool: p.address,
      quoteMint: p.pool.quoteMint,
      borrowerQuote: await findTokenAccount(borrower.address, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS),
      collateralMint: r.mint,
      borrowerCollateral: await findTokenAccount(borrower.address, r.mint, TOKEN_2022_PROGRAM_ADDRESS),
      loan,
      collateralVault: vault,
      loanProceeds: await findTokenAccount(loan, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS),
      roots: r.proof.latest.rootsAccount,
      quoteTokenProgram: TOKEN_PROGRAM_ADDRESS,
      collateralTokenProgram: TOKEN_2022_PROGRAM_ADDRESS,
      units: r.units,
      principal: r.principal,
      maturityEpochs: r.maturityEpochs,
      proven: { root: r.proof.proven.root, snapshot: r.proof.proven.snapshotBytes, proof: r.proof.proven.proof },
    }),
  ];
}

/** The token accounts a loan settles through (`LoanTokens`), shared by repay and sync. */
async function findLoanTokens(p: PoolAt, loanAddress: Address, loan: Loan) {
  const [poolAuthority] = await findPoolAuthorityPda({ pool: p.address });
  return {
    pool: p.address,
    loan: loanAddress,
    poolAuthority,
    quoteVault: await findTokenAccount(poolAuthority, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS),
    collateralVault: await findTokenAccount(loanAddress, loan.collateralMint, TOKEN_2022_PROGRAM_ADDRESS),
    loanProceeds: await findTokenAccount(loanAddress, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS),
    borrowerQuote: await findTokenAccount(loan.borrower, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS),
    quoteTokenProgram: TOKEN_PROGRAM_ADDRESS,
    collateralTokenProgram: TOKEN_2022_PROGRAM_ADDRESS,
    quoteMint: p.pool.quoteMint,
    collateralMint: loan.collateralMint,
  };
}

/** Anyone may repay a loan; what is left once nothing is owed goes to its borrower. */
export async function buildRepay(payer: TransactionSigner, p: PoolAt, loanAddress: Address, loan: Loan, amount: bigint): Promise<Instruction[]> {
  return [
    // The borrower's USDC account receives what is left once nothing is owed.
    await getCreateAssociatedTokenIdempotentInstructionAsync({ payer, owner: loan.borrower, mint: p.pool.quoteMint, tokenProgram: TOKEN_PROGRAM_ADDRESS }),
    await getRepayInstructionAsync({
      ...(await findLoanTokens(p, loanAddress, loan)),
      payer,
      payerQuote: await findTokenAccount(payer.address, p.pool.quoteMint, TOKEN_PROGRAM_ADDRESS),
      amount,
    }),
  ];
}

export async function buildSyncLoan(payer: TransactionSigner, p: PoolAt, loanAddress: Address, loan: Loan): Promise<Instruction[]> {
  return [
    await getCreateAssociatedTokenIdempotentInstructionAsync({ payer, owner: loan.borrower, mint: p.pool.quoteMint, tokenProgram: TOKEN_PROGRAM_ADDRESS }),
    await getSyncLoanInstructionAsync(await findLoanTokens(p, loanAddress, loan)),
  ];
}

export async function buildWithdrawUnits(borrower: TransactionSigner, p: PoolAt, loan: Loan): Promise<Instruction[]> {
  return [
    await getWithdrawUnitsInstructionAsync({
      borrower,
      pool: p.address,
      borrowerCollateral: await findTokenAccount(borrower.address, loan.collateralMint, TOKEN_2022_PROGRAM_ADDRESS),
      collateralMint: loan.collateralMint,
      collateralTokenProgram: TOKEN_2022_PROGRAM_ADDRESS,
    }),
  ];
}

function withWritableAccounts(instruction: Instruction, accounts: Address[]): Instruction {
  return {
    ...instruction,
    accounts: [...(instruction.accounts ?? []), ...accounts.map((a) => ({ address: a, role: AccountRole.WRITABLE }))],
  };
}

/** Marks `loans`, all against the token `proof` proves, from the pool's latest root. */
async function buildMarkLoans(payer: TransactionSigner, p: PoolAt, proof: TokenProof, loans: Address[]): Promise<Instruction[]> {
  const mark = getMarkLoansInstruction({
    pool: p.address,
    roots: proof.latest.rootsAccount,
    snapshots: [{ root: proof.proven.root, snapshot: proof.proven.snapshotBytes, proof: proof.proven.proof }],
  });
  return [await buildSettleEpoch(payer, p), withWritableAccounts(mark, loans)];
}

/**
 * Marks a token's loans in as few v0 transactions as hold them, which every
 * wallet signs. Each carries the token's proof again, so a longer proof leaves
 * room for fewer loans.
 */
export async function buildMarkLoanBatches(payer: TransactionSigner, p: PoolAt, proof: TokenProof, loans: Address[]): Promise<Instruction[][]> {
  const batches: Instruction[][] = [];
  let start = 0;
  while (start < loans.length) {
    let end = start + 1;
    // A proof too long for even one loan in a v0 transaction goes one loan at a
    // time, which `sendInstructions` sends as v1.
    let batch = await buildMarkLoans(payer, p, proof, loans.slice(start, end));
    while (end < loans.length && isWithinOneTransaction(payer, batch)) {
      const next = await buildMarkLoans(payer, p, proof, loans.slice(start, end + 1));
      if (!isWithinOneTransaction(payer, next)) {
        break;
      }
      batch = next;
      end++;
    }
    batches.push(batch);
    start = end;
  }
  return batches;
}

/** Marks stale the loans nobody marked, once the marking window has closed. */
export async function buildMarkStale(payer: TransactionSigner, p: PoolAt, loans: Address[]): Promise<Instruction[]> {
  return [await buildSettleEpoch(payer, p), withWritableAccounts(getMarkStaleInstruction({ pool: p.address }), loans)];
}
