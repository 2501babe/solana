use {
    crate::account_loader::AccountRetrievalCallback,
    solana_svm_transaction::svm_transaction::SVMTransaction,
};

// HANA OK WTF what am i doing again
// im making a balance collector trait so that we can call a fn in svm
// we uhhhh. trait. two fns. inspect pre inspect post
// pre takes mut self, loader, tx ref, load result
// post takes mut self, loader, tx ref, proc result
// these can be normal functions and call into the trait object
// trait provides a store balances function that takes mut self and lamp and token vec
//
// as an alternative we could have the trait provide the pre and post functions
// that requires exposing loader tho. doing it this way is better
//
// ok we cant accept balances bc it requires token bullshit from transaction-status
// we dont need to expose AccountLoader bc we have atrait now
// our split fns need to at least determine what to do... hm
//
// we want svm fns that take loaded tx and proc result. match on them
// man. we dont even necessarily want to call it twice in all cases
// * load aborted: insert dummy vecs. order and length must be preserved
// * fees only: get lamports before and after, but could reuse the before values except feepayer
// * loaded: get lamports and tokens before, might need to load mints
// then on the execution result
// * failed: can do similar to fees only after case, just reuse existing and update feepayer
// * succeeded: get lamports and tokens after, might need to load mints
//
// can we get away with collect pre and collect post, with a bool for tokens?
// and i guess a third skip collection function. then for load results we rely on our natural matching
// and for execution result its the same, we already check is successful
//
// there are more optimal ways (skipping non-feepayer on fail) but for now do naive

pub trait TransactionBatchBalanceCollector<CB, TX>
where
    CB: AccountRetrievalCallback,
    TX: SVMTransaction,
{
    fn collect_pre_balances(&mut self, loader: &mut CB, transaction: &TX, include_tokens: bool);

    fn collect_post_balances(&mut self, loader: &mut CB, transaction: &TX, include_tokens: bool);

    fn skip_transaction(&mut self);
}

pub struct DummyBalanceCollector {}

impl<AR: AccountRetrievalCallback, TX: SVMTransaction> TransactionBatchBalanceCollector<AR, TX>
    for DummyBalanceCollector
{
    fn collect_pre_balances(&mut self, _loader: &mut AR, _transaction: &TX, _include_tokens: bool) {
    }

    fn collect_post_balances(
        &mut self,
        _loader: &mut AR,
        _transaction: &TX,
        _include_tokens: bool,
    ) {
    }

    fn skip_transaction(&mut self) {}
}
