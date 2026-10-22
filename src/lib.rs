use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

declare_id!("Dbdha1ivCfnBAW2Tv8n44Mjefv11DkPRp7y2S2R38gp1");

// A tiny pizza shop on Solana.
// 1. The owner opens a shop with a price (in lamports) for each of 4 pizzas.
// 2. Customers order pizzas: the payment goes into the shop's vault PDA and an Order account records it.
// 3. The owner marks orders ready and withdraws the takings from the vault.
// Menu: 0 Margherita, 1 Pepperoni, 2 Veggie, 3 Hawaiian.
const MENU_SIZE: usize = 4;
const MIN_PRICE: u64 = 1_000_000; // 0.001 SOL, so every payment can fund the vault account

#[program]
pub mod pizza_shop {
    use super::*;

    pub fn open_shop(ctx: Context<OpenShop>, prices: [u64; MENU_SIZE]) -> Result<()> {
        require!(prices.iter().all(|p| *p >= MIN_PRICE), PizzaError::PriceTooLow);
        let s = &mut ctx.accounts.shop;
        s.owner = ctx.accounts.owner.key();
        s.prices = prices;
        s.next_order_id = 0;
        s.pizzas_sold = 0;
        s.bump = ctx.bumps.shop;
        s.vault_bump = ctx.bumps.vault;
        Ok(())
    }

    pub fn set_price(ctx: Context<OwnerOnly>, kind: u8, price: u64) -> Result<()> {
        require!((kind as usize) < MENU_SIZE, PizzaError::UnknownPizza);
        require!(price >= MIN_PRICE, PizzaError::PriceTooLow);
        ctx.accounts.shop.prices[kind as usize] = price;
        Ok(())
    }

    pub fn order_pizza(ctx: Context<OrderPizza>, kind: u8, quantity: u8) -> Result<()> {
        require!((kind as usize) < MENU_SIZE, PizzaError::UnknownPizza);
        require!(quantity >= 1 && quantity <= 20, PizzaError::BadQuantity);

        let shop = &mut ctx.accounts.shop;
        let paid = shop.prices[kind as usize]
            .checked_mul(quantity as u64)
            .ok_or(PizzaError::Overflow)?;

        let o = &mut ctx.accounts.order;
        o.shop = shop.key();
        o.customer = ctx.accounts.customer.key();
        o.id = shop.next_order_id;
        o.kind = kind;
        o.quantity = quantity;
        o.paid = paid;
        o.ready = false;
        o.bump = ctx.bumps.order;

        shop.next_order_id = shop.next_order_id.checked_add(1).ok_or(PizzaError::Overflow)?;
        shop.pizzas_sold = shop.pizzas_sold.checked_add(quantity as u64).ok_or(PizzaError::Overflow)?;

        transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.customer.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            paid,
        )
    }

    pub fn mark_ready(ctx: Context<MarkReady>) -> Result<()> {
        require!(!ctx.accounts.order.ready, PizzaError::AlreadyReady);
        ctx.accounts.order.ready = true;
        Ok(())
    }

    // Owner takes money out of the vault. The vault must keep 0 or at least the rent minimum.
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let shop_key = ctx.accounts.shop.key();
        let seeds: &[&[u8]] = &[b"vault", shop_key.as_ref(), &[ctx.accounts.shop.vault_bump]];
        transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.owner.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )
    }
}

#[account]
#[derive(InitSpace)]
pub struct Shop {
    pub owner: Pubkey,
    pub prices: [u64; MENU_SIZE],
    pub next_order_id: u64,
    pub pizzas_sold: u64,
    pub bump: u8,
    pub vault_bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Order {
    pub shop: Pubkey,
    pub customer: Pubkey,
    pub id: u64,
    pub kind: u8,
    pub quantity: u8,
    pub paid: u64,
    pub ready: bool,
    pub bump: u8,
}

#[derive(Accounts)]
pub struct OpenShop<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        init, payer = owner, space = 8 + Shop::INIT_SPACE,
        seeds = [b"shop", owner.key().as_ref()], bump
    )]
    pub shop: Account<'info, Shop>,
    #[account(mut, seeds = [b"vault", shop.key().as_ref()], bump)]
    pub vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OwnerOnly<'info> {
    pub owner: Signer<'info>,
    #[account(mut, has_one = owner)]
    pub shop: Account<'info, Shop>,
}

#[derive(Accounts)]
pub struct OrderPizza<'info> {
    #[account(mut)]
    pub customer: Signer<'info>,
    #[account(mut)]
    pub shop: Account<'info, Shop>,
    #[account(
        init, payer = customer, space = 8 + Order::INIT_SPACE,
        seeds = [b"order", shop.key().as_ref(), &shop.next_order_id.to_le_bytes()], bump
    )]
    pub order: Account<'info, Order>,
    #[account(mut, seeds = [b"vault", shop.key().as_ref()], bump = shop.vault_bump)]
    pub vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MarkReady<'info> {
    pub owner: Signer<'info>,
    #[account(has_one = owner)]
    pub shop: Account<'info, Shop>,
    #[account(mut, has_one = shop)]
    pub order: Account<'info, Order>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(has_one = owner)]
    pub shop: Account<'info, Shop>,
    #[account(mut, seeds = [b"vault", shop.key().as_ref()], bump = shop.vault_bump)]
    pub vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum PizzaError {
    #[msg("Price must be at least 0.001 SOL")]
    PriceTooLow,
    #[msg("Unknown pizza: pick 0 to 3")]
    UnknownPizza,
    #[msg("Quantity must be between 1 and 20")]
    BadQuantity,
    #[msg("Order is already ready")]
    AlreadyReady,
    #[msg("Arithmetic overflow")]
    Overflow,
}
