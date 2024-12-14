//! # Fundamentals lesson 5: XCM Executor
//!
//! Create your own executor for XCM.

use super::holding::*;

use sp_std::{marker::PhantomData, prelude::*};
use xcm::latest::prelude::*;
use xcm_executor::traits::{ProcessTransaction, TransactAsset};

pub trait XcmConfig {
    /// How to withdraw and deposit an asset.
    type AssetTransactor: TransactAsset;
    /// Transactional processor for XCM instructions.
    type TransactionalProcessor: ProcessTransaction;
}

/// The heart of the XCM Virtual Machine.
pub struct XcmExecutor<Config: XcmConfig> {
    /// The asset holding registrar, where we keep track of assets being processed by the XCM
    /// Executor.
    pub holding: AssetsInHolding,
    /// Contextual data pertaining to a specific list of XCM instructions. Most relevant the
    /// `origin` of the XCM Message.
    pub context: XcmContext,
    /// Just a placeholder to allow Rust to let us keep `Config`.
    _config: PhantomData<Config>,
}

/// The implementation of the XCM Executor and how it processes XCM.
impl<Config: XcmConfig> XcmExecutor<Config> {
    /// Crete an initialize a new XCM Executor.
    pub fn new(origin: impl Into<Location>) -> Self {
        let origin = origin.into();
        // In our version of the XCM Executor, we ignore `message_id` and `topic`.
        let context = XcmContext {
            origin: Some(origin),
            message_id: Default::default(),
            topic: None
        };
        Self { holding: Default::default(), context, _config: PhantomData }
    }

    /// Process an entire XCM program, instruction by instruction.
    pub fn process(&mut self, xcm: Xcm<()>) -> Result<(), XcmError> {
        log::trace!(target: "xcm::process", "xcm: {:?}", xcm);
        for instruction in xcm.0.into_iter() {
            self.process_instruction(instruction)?;
        }
        Ok(())
    }

    /// Simple helper function to access the `origin` from the XCM Executor `context`.
    pub fn origin_ref(&self) -> Option<&Location> {
        self.context.origin.as_ref()
    }

    /// Process a single XCM instruction, mutating the state of the XCM virtual machine.
    fn process_instruction(&mut self, instr: Instruction<()>) -> Result<(), XcmError> {
        log::trace!(target: "xcm::process_instruction", "=== {:?}", instr);
        match instr {
            // Clear the origin.
            //
            // This may be used by the XCM author to ensure that later instructions cannot command
            // the authority of the origin (e.g. if they are being relayed from an untrusted
            // source, as often the case with `ReserveAssetDeposited`).
            ClearOrigin => {
                self.context.origin = None;
                Ok(())
            },
            // Appends `who` to the current XCM Executor `origin` location.
            DescendOrigin(who) => {
                let _ = self.context.origin.as_mut() // is option, can None
                    .ok_or(XcmError::BadOrigin)?
                    .append_with(who)
                    .map_err(|_| XcmError::LocationFull);
                
                Ok(())
            },
            // Withdraw asset(s) (`assets`) from the ownership of `origin` and place equivalent
            // assets under the ownership of `beneficiary`.
            //
            // - `assets`: The asset(s) to be withdrawn.
            // - `beneficiary`: The new owner for the assets.
            TransferAsset { assets, beneficiary } => {
                /* TODO:
                    - Process everything inside a `TransactionalProcessor`.
                    - Get the `origin` from `self.origin_ref()`.
                    - For each `asset` in `assets`
                        - Use `AssetTransactor` and `transfer_asset` to the `beneficiary`.
                    - If everything works well, return `Ok(())`
                */
                
                Config::TransactionalProcessor::process(|| {
                    let origin = self.origin_ref().ok_or(XcmError::BadOrigin)?;
                    for asset in assets.inner() {
                        Config::AssetTransactor::transfer_asset(
                            &asset,
                            origin,
                            &beneficiary,
                            &self.context
                        )?;
                    }
                    Ok(())
                })
            },
            // Withdraw asset(s) (`assets`) from the ownership of `origin` and place them into the
            // Holding Register.
            //
            // - `assets`: The asset(s) to be withdrawn into holding.
            WithdrawAsset(assets) => {
                 /* TODO:
                    - Process everything inside a `TransactionalProcessor`.
                    - Get the `origin` or return `XcmError::BadOrigin`.
                    - For each `asset` in `assets`
                        - Use the `AssetTransactor` to `withdraw_asset`.
                    - `and_then`, if everything goes okay...
                    - `subsume_assets` into the `self.holding`
                */

                Config::TransactionalProcessor::process(|| {
                    let origin = self.origin_ref().ok_or(XcmError::BadOrigin)?;
                    for asset in assets.inner() {
                        Config::AssetTransactor::withdraw_asset(
                            &asset,
                            origin,
                            Some(&self.context)
                        )?;
                    }
                    Ok(())
                })
                .and_then(|_| {
                    // ...and place into holding.
                    self.holding.subsume_assets(assets.into());
                    Ok(())
                })
            },
            // Reduce Holding by up to the given assets.
            //
            // Holding is reduced by as much as possible up to the assets in the parameter. It is
            // not an error if the Holding does not contain the assets (to make this an error, use
            // `ExpectAsset` prior).
            BurnAsset(assets) => {
                self.holding.saturating_take(assets.into());
                Ok(())
            },
            // Remove the asset(s) (`assets`) from the Holding Register and place equivalent assets
            // under the ownership of `beneficiary` within this consensus system.
            //
            // - `assets`: The asset(s) to remove from holding.
            // - `beneficiary`: The new owner for the assets.
            DepositAsset { assets, beneficiary } => {
                /* TODO:
                    - Make a clone of `self.holding` into a variable `old_holding`.
                    - Start a new `TransactionalProcessor`, storing the `result`.
                        - `saturating_take` the `assets` into a variable `deposited.
                        - For `asset` in `deposited`
                            - Use `AssetTransactor` to `deposit_asset` to the `beneficiary`.
                    - If anything goes wrong, we should reset `self.holding` to `old_holding`.
                    - Return the `result`
                */
                let old_holding = self.holding.clone();
                let result = Config::TransactionalProcessor::process(|| {
                    let deposited = self.holding.saturating_take(assets);
                    for asset in deposited.into_assets_iter() {
                        Config::AssetTransactor::deposit_asset(
                            &asset,
                            &beneficiary,
                            Some(&self.context),
                        )?;
                    }
                    Ok(())
                });

                // If we were unable to execute `deposit_asset` in the `AssetTransactor`, we reset
                // the XCM Executor holding registrar since no operations took place.
                if Config::TransactionalProcessor::IS_TRANSACTIONAL && result.is_err() {
                    self.holding = old_holding;
                }
                result
            },
            // Asset(s) (`assets`) have been destroyed on the `origin` system and equivalent assets
            // should be created and placed into the Holding Register.
            //
            // - `assets`: The asset(s) that are minted into the Holding Register.
            ReceiveTeleportedAsset(assets) => {
                /* TODO:
                    - Process everything inside `TransactionalProcessor`.
                        - Get the `origin` or return `XcmError::BadOrigin`.
                        - For `asset` in `assets`:
                            - Use `AssetTransactor` to see if we `can_check_in`.
                            - Then actually `check_in` those assets.
                    - `and_then`, if everything goes okay...
                    - `subsume_assets` into the `self.holding`.
                */
                Config::TransactionalProcessor::process(|| {
                    let origin = self.origin_ref().ok_or(XcmError::BadOrigin)?;
                    for asset in assets.inner() {
                        // This ensures that only "proper origin" can mint asset
                        Config::AssetTransactor::can_check_in(
                            origin,
                            asset,
                            &self.context,
                        )?;
                        Config::AssetTransactor::check_in(
                            origin,
                            asset,
                            &self.context,
                        );
                    }
                    Ok(())
                })
                .and_then(|_| {
                    self.holding.subsume_assets(assets.into());
                    Ok(())
                })
            },
            // In this workshop, we won't be implementing every instruction, just the ones above...
            // Our executor will simply panic if you try to execute other instructions.
            _ => unimplemented!(),
        }
    }
}

/// A public trait allowing other systems to access and use the `XcmExecutor`.
pub trait ExecuteXcm {
    /// Execute an XCM from a given `origin`.
    fn execute(origin: impl Into<Location>, xcm: Xcm<()>) -> XcmResult;
}

impl<Config: XcmConfig> ExecuteXcm for XcmExecutor<Config> {
    /// Execute an XCM from a given `origin`.
    fn execute(origin: impl Into<Location>, xcm: Xcm<()>) -> XcmResult {
        log::trace!(target: "xcm::execute", "xcm: {:?}", xcm);
         /* TODO:
            - Convert the `origin` `into` a `Location`.
            - Create a new mutable instance of the XCM Executor as `vm`.
            - Use the `vm` to `process` the `xcm`.
        */
        let mut vm = Self::new(origin.into());
        vm.process(xcm)
    }
}
