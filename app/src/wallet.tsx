// Wallets the browser offers through the Wallet Standard (Phantom,
// Solflare, Backpack and the rest): no wallet is built in or preferred.

import { useWalletAccountTransactionSendingSigner } from '@solana/react';
import type { TransactionSendingSigner } from '@solana/kit';
import { useConnect, useDisconnect, useWallets, type UiWallet, type UiWalletAccount } from '@wallet-standard/react';
import { createContext, useContext, useState, type ReactNode } from 'react';

import { config } from './config';
import { shortenAddress } from './format';

type WalletState = {
  account?: UiWalletAccount;
  wallet?: UiWallet;
  select: (wallet: UiWallet | undefined, account: UiWalletAccount | undefined) => void;
};

const WalletContext = createContext<WalletState>({ select: () => {} });

export function WalletProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<{ wallet?: UiWallet; account?: UiWalletAccount }>({});
  return (
    <WalletContext.Provider value={{ ...state, select: (wallet, account) => setState({ wallet, account }) }}>
      {children}
    </WalletContext.Provider>
  );
}

export function useWalletAccount(): UiWalletAccount | undefined {
  return useContext(WalletContext).account;
}

/** Whether a wallet can sign and send on this app's chain. */
function isUsable(wallet: UiWallet): boolean {
  return (
    wallet.chains.includes(config.chain) &&
    wallet.features.includes('standard:connect') &&
    wallet.features.includes('solana:signAndSendTransaction')
  );
}

function ConnectOption({ wallet }: { wallet: UiWallet }) {
  const { select } = useContext(WalletContext);
  const [isConnecting, connect] = useConnect(wallet);
  const [error, setError] = useState<string>();
  return (
    <li>
      <button
        className="wallet-option"
        disabled={isConnecting}
        onClick={async () => {
          setError(undefined);
          try {
            const accounts = await connect();
            select(wallet, accounts[0]);
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
          }
        }}
      >
        {wallet.icon && <img src={wallet.icon} alt="" width={20} height={20} />}
        {isConnecting ? `Connecting to ${wallet.name}…` : wallet.name}
      </button>
      {error && <p className="error">{error}</p>}
    </li>
  );
}

function DisconnectButton({ wallet }: { wallet: UiWallet }) {
  const { account, select } = useContext(WalletContext);
  const [, disconnect] = useDisconnect(wallet);
  return (
    <button
      className="secondary"
      onClick={async () => {
        await disconnect().catch(() => {});
        select(undefined, undefined);
      }}
    >
      {account ? shortenAddress(account.address) : ''} · Disconnect
    </button>
  );
}

export function WalletButton() {
  const { wallet, account } = useContext(WalletContext);
  const wallets = useWallets().filter(isUsable);
  const [isOpen, setOpen] = useState(false);
  if (wallet && account) {
    return <DisconnectButton wallet={wallet} />;
  }
  return (
    <div className="wallet-menu">
      <button onClick={() => setOpen(!isOpen)}>Connect a wallet</button>
      {isOpen && (
        <div className="wallet-popover">
          {wallets.length === 0 ? (
            <p className="muted">No wallet in this browser signs on {config.network}. Install one that supports the Wallet Standard, such as Phantom, Solflare or Backpack.</p>
          ) : (
            <ul>
              {wallets.map((w) => (
                <ConnectOption key={w.name} wallet={w} />
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

function SignerFor({ account, children }: { account: UiWalletAccount; children: (signer: TransactionSendingSigner) => ReactNode }) {
  const signer = useWalletAccountTransactionSendingSigner(account, config.chain);
  return <>{children(signer)}</>;
}

/** Renders `children` with the connected wallet's signer, or asks to connect one. */
export function WithSigner({ children, prompt }: { children: (signer: TransactionSendingSigner) => ReactNode; prompt: string }) {
  const account = useWalletAccount();
  if (!account) {
    return (
      <div className="notice">
        <p>{prompt}</p>
        <WalletButton />
      </div>
    );
  }
  return <SignerFor account={account}>{children}</SignerFor>;
}
