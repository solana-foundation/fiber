extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    Expr, Ident, LitInt, Path, Token,
};

// --- Offset computation (mirrors Solana's entrypoint buffer layout) ---

const ACCT_HEADER: usize = 88; // dup(1) + flags(3) + pad(4) + pubkey(32) + owner(32) + lamports(8) + data_len(8)
const MAX_DATA_INCREASE: usize = 10240;
const BPF_ALIGN: usize = 8;

/// Compute the byte offset where an account's region starts in the input buffer.
fn account_region_start(preceding_data_sizes: &[usize]) -> usize {
    let mut offset = 8; // skip num_accounts u64
    for &data_size in preceding_data_sizes {
        offset += ACCT_HEADER + data_size + MAX_DATA_INCREASE;
        offset = (offset + BPF_ALIGN - 1) & !(BPF_ALIGN - 1);
        offset += 8; // rent_epoch
    }
    offset
}

/// Offsets for a specific account relative to input pointer.
struct AccountOffsets {
    start: usize,
    pubkey: usize,
    lamports: usize,
    data: usize,
}

fn compute_account_offsets(preceding_data_sizes: &[usize]) -> AccountOffsets {
    let start = account_region_start(preceding_data_sizes);
    AccountOffsets {
        start,
        pubkey: start + 8,    // after dup+flags+pad
        lamports: start + 72, // after pubkey(32) + owner(32)
        data: start + 88,     // after lamports(8) + data_len(8)
    }
}

fn compute_ix_data_offset(all_data_sizes: &[usize]) -> usize {
    let after_last = account_region_start(all_data_sizes);
    after_last + 8 // skip instruction_data_len u64
}

// --- Parsing ---

enum AccountType {
    Signer(usize),   // data_size
    Writable(usize), // data_size
}

impl Parse for AccountType {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let content;
        syn::parenthesized!(content in input);
        let size: LitInt = content.parse()?;
        let size_val = size.base10_parse::<usize>()?;
        match name.to_string().as_str() {
            "signer" => Ok(AccountType::Signer(size_val)),
            "writable" => Ok(AccountType::Writable(size_val)),
            _ => Err(syn::Error::new(
                name.span(),
                "expected `signer` or `writable`",
            )),
        }
    }
}

enum InstructionMode {
    Fixed(Vec<AccountType>),
    Variable { signer_check: bool },
}

struct InstructionArm {
    discriminator: Expr,
    mode: InstructionMode,
    handler: Path,
}

impl Parse for InstructionArm {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Parse #[fixed(...)] or #[variable] or #[variable(signer_check)]
        input.parse::<Token![#]>()?;
        let attr_content;
        syn::bracketed!(attr_content in input);
        let attr_name: Ident = attr_content.parse()?;

        let mode = match attr_name.to_string().as_str() {
            "fixed" => {
                let accounts_content;
                syn::parenthesized!(accounts_content in attr_content);
                let accounts: Punctuated<AccountType, Token![,]> =
                    accounts_content.parse_terminated(AccountType::parse, Token![,])?;
                InstructionMode::Fixed(accounts.into_iter().collect())
            }
            "variable" => {
                let signer_check = if attr_content.peek(syn::token::Paren) {
                    let sc_content;
                    syn::parenthesized!(sc_content in attr_content);
                    let flag: Ident = sc_content.parse()?;
                    flag == "signer_check"
                } else {
                    false
                };
                InstructionMode::Variable { signer_check }
            }
            _ => {
                return Err(syn::Error::new(
                    attr_name.span(),
                    "expected `fixed` or `variable`",
                ))
            }
        };

        // Parse: DISC => Handler,
        let discriminator: Expr = input.parse()?;
        input.parse::<Token![=>]>()?;
        let handler: Path = input.parse()?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }

        Ok(InstructionArm {
            discriminator,
            mode,
            handler,
        })
    }
}

struct FiberEntrypoint {
    arms: Vec<InstructionArm>,
}

impl Parse for FiberEntrypoint {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut arms = Vec::new();
        while !input.is_empty() {
            arms.push(input.parse()?);
        }
        Ok(FiberEntrypoint { arms })
    }
}

// --- Code generation ---

fn gen_fixed_arm(arm: &InstructionArm, accounts: &[AccountType]) -> TokenStream2 {
    let disc = &arm.discriminator;
    let handler = &arm.handler;

    let data_sizes: Vec<usize> = accounts
        .iter()
        .map(|a| match a {
            AccountType::Signer(s) | AccountType::Writable(s) => *s,
        })
        .collect();

    // Generate const offsets for each account
    let mut offset_consts = Vec::new();
    let mut acct_vars = Vec::new();

    for (i, _acct) in accounts.iter().enumerate() {
        let preceding: Vec<usize> = data_sizes[..i].to_vec();
        let offsets = compute_account_offsets(&preceding);

        let pubkey_name = Ident::new(
            &format!("__ACCT{}_PUBKEY", i),
            proc_macro2::Span::call_site(),
        );
        let lamports_name = Ident::new(
            &format!("__ACCT{}_LAMPORTS", i),
            proc_macro2::Span::call_site(),
        );
        let data_name = Ident::new(&format!("__ACCT{}_DATA", i), proc_macro2::Span::call_site());
        let header_name = Ident::new(
            &format!("__ACCT{}_HEADER", i),
            proc_macro2::Span::call_site(),
        );

        let pubkey_val = offsets.pubkey;
        let lamports_val = offsets.lamports;
        let data_val = offsets.data;
        let header_val = offsets.start;

        offset_consts.push(quote! {
            const #pubkey_name: usize = #pubkey_val;
            const #lamports_name: usize = #lamports_val;
            const #data_name: usize = #data_val;
            const #header_name: usize = #header_val;
        });

        acct_vars.push(i);
    }

    let ix_offset = compute_ix_data_offset(&data_sizes);
    let ix_offset_const = quote! { const __IX_DATA: usize = #ix_offset; };

    // Signer check for first signer account
    let signer_check = accounts.iter().enumerate().find_map(|(i, a)| {
        if let AccountType::Signer(_) = a {
            let header_name = Ident::new(
                &format!("__ACCT{}_HEADER", i),
                proc_macro2::Span::call_site(),
            );
            Some(quote! {
                // Signer check: dup(0xFF) | is_signer(0x01) = 0x01FF as LE u16
                if *(input.add(#header_name) as *const u16) != 0x01FF {
                    fiber::fail(1);
                }
            })
        } else {
            None
        }
    });

    quote! {
        #disc => {
            #(#offset_consts)*
            #ix_offset_const
            #signer_check
            #handler(input);
        }
    }
}

fn gen_variable_arm(arm: &InstructionArm, signer_check: bool) -> TokenStream2 {
    let disc = &arm.discriminator;
    let handler = &arm.handler;

    let check = if signer_check {
        quote! {
            if !__accounts[0].is_signer() {
                fiber::fail(1);
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #disc => {
            #check
            #handler(input, __accounts, __num, __ix_offset);
        }
    }
}

// --- Main macro ---

#[proc_macro]
pub fn fastlane(input: TokenStream) -> TokenStream {
    let parsed = parse_macro_input!(input as FiberEntrypoint);

    let mut fixed_arms = Vec::new();
    let mut variable_arms = Vec::new();
    let mut has_variable = false;

    for arm in &parsed.arms {
        match &arm.mode {
            InstructionMode::Fixed(accounts) => {
                fixed_arms.push(gen_fixed_arm(arm, accounts));
            }
            InstructionMode::Variable { signer_check } => {
                has_variable = true;
                variable_arms.push(gen_variable_arm(arm, *signer_check));
            }
        }
    }

    let variable_block = if has_variable {
        quote! {
            // Pinocchio's const UNINIT pattern — compiler recognizes and optimizes
            const __UNINIT: core::mem::MaybeUninit<solana_account_view::AccountView> =
                core::mem::MaybeUninit::uninit();
            let mut __acct_buf = [__UNINIT; 64];

            let (_, __num, __ix_data) =
                pinocchio::entrypoint::deserialize::<64>(input, &mut __acct_buf);

            let __accounts = core::slice::from_raw_parts_mut(
                __acct_buf.as_mut_ptr() as *mut solana_account_view::AccountView,
                __num,
            );

            let __disc = __ix_data[0];
            let __ix_offset = (__ix_data.as_ptr() as usize) - (input as usize);

            match __disc {
                #(#variable_arms)*
                _ => fiber::fail(7),
            }
        }
    } else {
        quote! { fiber::fail(7); }
    };

    // For fixed arms, we read discriminator at a known offset.
    // All fixed arms must have the same account layout for the disc offset to be the same.
    // In practice, our fixed instructions all have (signer(0), writable(42)) so disc is at 0x5100.
    // We compute it from the first fixed arm.
    let fixed_block = if !fixed_arms.is_empty() {
        // Get the disc offset and expected account count from the first fixed arm
        let first_fixed = parsed
            .arms
            .iter()
            .find(|a| matches!(a.mode, InstructionMode::Fixed(_)));
        let (disc_offset, expected_num_accounts) = if let Some(arm) = first_fixed {
            if let InstructionMode::Fixed(accounts) = &arm.mode {
                let sizes: Vec<usize> = accounts
                    .iter()
                    .map(|a| match a {
                        AccountType::Signer(s) | AccountType::Writable(s) => *s,
                    })
                    .collect();
                (compute_ix_data_offset(&sizes), accounts.len())
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

        let expected_num = expected_num_accounts as u64;

        quote! {
            let __num_accounts = *(input as *const u64);
            if __num_accounts == #expected_num {
                // Fixed layout — disc at compile-time offset, no parsing
                let __disc = *input.add(#disc_offset);
                match __disc {
                    #(#fixed_arms)*
                    _ => fiber::fail(7),
                }
            } else {
                // Variable layout — parse accounts, dispatch
                #variable_block
            }
        }
    } else {
        variable_block
    };

    let output = quote! {
        // Global allocator required by pinocchio's deserializer (SBF only)
        #[cfg(target_os = "solana")]
        #[global_allocator]
        static ALLOCATOR: pinocchio::entrypoint::BumpAllocator = unsafe {
            pinocchio::entrypoint::BumpAllocator::new_unchecked(
                pinocchio::entrypoint::HEAP_START_ADDRESS as usize,
                pinocchio::entrypoint::MAX_HEAP_LENGTH as usize,
            )
        };

        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *mut u8) -> u64 {
            #fixed_block
            0
        }
    };

    output.into()
}
