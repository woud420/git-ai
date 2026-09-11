use super::*;

/// Test 9: service.py process_payment — feature (C4) AI implements a 20-line
/// function body; main also implements the same function (12 lines).
/// AI resolution produces a 25-line merged implementation (all .ai()).
/// Non-conflict commits: C1 models.py, C2 validators.py, C3 exceptions.py, C5 utils.py.
#[test]
fn test_conflict_ai_resolves_complex_function_with_error_handling() {
    run_test_conflict_ai_resolves_complex_function_with_error_handling(
        HumanContextAttribution::Known,
    );
}

#[test]
fn test_conflict_ai_resolves_complex_function_with_error_handling_standard_human() {
    run_test_conflict_ai_resolves_complex_function_with_error_handling(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_complex_function_with_error_handling(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: service.py with a function stub (human)
    repo.commit_untracked_file(
        "service.py",
        "def process_payment(amount, card):\n    pass\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: human implements process_payment differently → will conflict
    repo.commit_untracked_file("service.py",
        "def process_payment(amount, card):\n    if amount <= 0:\n        raise ValueError('amount must be positive')\n    return {'status': 'ok', 'amount': amount}\n",
        "main: implement process_payment",
    );
    repo.commit_untracked_file("tests/test_service.py",
        "from service import process_payment\ndef test_basic(): assert process_payment(10, '4111')['status'] == 'ok'\n",
        "main: add service tests",
    );
    repo.commit_untracked_file(
        "requirements.txt",
        "stripe==5.0.0\nrequests==2.31.0\n",
        "main: add requirements",
    );
    repo.commit_untracked_file(
        ".env.example",
        "STRIPE_KEY=sk_test_xxx\nDATABASE_URL=sqlite:///dev.db\n",
        "main: add .env.example",
    );
    repo.commit_untracked_file(
        "Makefile",
        "test:\n\tpython -m pytest\nlint:\n\tflake8 .\n.PHONY: test lint\n",
        "main: add Makefile",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates models.py (8 AI lines)
    let mut models = repo.filename("models.py");
    models.set_contents(crate::lines![
        "from dataclasses import dataclass, field".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class PaymentResult:".ai(),
        "    status: str".ai(),
        "    transaction_id: str".ai(),
        "    amount: float".ai(),
        "    error: str = ''".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add PaymentResult model")
        .unwrap();

    // C2: AI creates validators.py (8 AI lines)
    let mut validators = repo.filename("validators.py");
    validators.set_contents(crate::lines![
        "import re".ai(),
        "".ai(),
        "def validate_card(card: str) -> bool:".ai(),
        "    return bool(re.match(r'^[0-9]{13,19}$', card.replace(' ', '')))".ai(),
        "".ai(),
        "def validate_amount(amount: float) -> bool:".ai(),
        "    return isinstance(amount, (int, float)) and 0 < amount <= 1_000_000".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add payment validators")
        .unwrap();

    // C3: AI creates exceptions.py (8 AI lines)
    let mut exceptions = repo.filename("exceptions.py");
    exceptions.set_contents(crate::lines![
        "class PaymentError(Exception):".ai(),
        "    def __init__(self, msg: str, code: int = 400):".ai(),
        "        super().__init__(msg)".ai(),
        "        self.code = code".ai(),
        "".ai(),
        "class CardDeclinedError(PaymentError):".ai(),
        "    def __init__(self): super().__init__('Card declined', 402)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add payment exceptions")
        .unwrap();

    // C4: AI implements process_payment with 20 lines — WILL CONFLICT
    let mut service = repo.filename("service.py");
    service.set_contents(crate::lines![
        human_context.expected_line("def process_payment(amount, card):"),
        "    from validators import validate_amount, validate_card".ai(),
        "    from exceptions import PaymentError, CardDeclinedError".ai(),
        "    import logging".ai(),
        "    logger = logging.getLogger(__name__)".ai(),
        "    logger.info(f'Processing payment: amount={amount}')".ai(),
        "    if not validate_amount(amount):".ai(),
        "        raise PaymentError(f'Invalid amount: {amount}')".ai(),
        "    if not validate_card(card):".ai(),
        "        raise PaymentError(f'Invalid card number')".ai(),
        "    if str(card).startswith('0000'):".ai(),
        "        raise CardDeclinedError()".ai(),
        "    transaction_id = f'txn_{hash(card + str(amount)) % 10**9}'".ai(),
        "    logger.info(f'Payment successful: {transaction_id}')".ai(),
        "    return {'status': 'ok', 'transaction_id': transaction_id, 'amount': amount}".ai(),
        "    # end process_payment".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 AI implements process_payment")
        .unwrap();

    // C5: AI creates utils.py (8 AI lines)
    let mut utils = repo.filename("utils.py");
    utils.set_contents(crate::lines![
        "def mask_card(card: str) -> str:".ai(),
        "    digits = card.replace(' ', '')".ai(),
        "    return '*' * (len(digits) - 4) + digits[-4:]".ai(),
        "".ai(),
        "def format_amount(amount: float) -> str:".ai(),
        "    return f'${amount:.2f}'".ai(),
        "".ai(),
        "def generate_receipt(result: dict) -> str: return f\"Receipt: {result['transaction_id']} {result['amount']}\"".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add payment utils")
        .unwrap();

    // Rebase — C4 will conflict on service.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on service.py at C4"
    );

    // AI resolves: 25-line merged implementation (all .ai() except function signature line)
    let mut conflict_service = repo.filename("service.py");
    conflict_service.set_contents(crate::lines![
        human_context.expected_line("def process_payment(amount, card):"),
        "    from validators import validate_amount, validate_card".ai(),
        "    from exceptions import PaymentError, CardDeclinedError".ai(),
        "    from models import PaymentResult".ai(),
        "    import logging".ai(),
        "    logger = logging.getLogger(__name__)".ai(),
        "    logger.info(f'Processing: amount={amount} card=***{str(card)[-4:]}')".ai(),
        "    if amount <= 0:".ai(),
        "        raise ValueError('amount must be positive')".ai(),
        "    if not validate_amount(amount):".ai(),
        "        raise PaymentError(f'Amount out of range: {amount}')".ai(),
        "    if not validate_card(card):".ai(),
        "        raise PaymentError('Invalid card number format')".ai(),
        "    if str(card).startswith('0000'):".ai(),
        "        raise CardDeclinedError()".ai(),
        "    transaction_id = f'txn_{hash(str(card) + str(amount)) % 10**9}'".ai(),
        "    logger.info(f'Payment OK: txn={transaction_id}')".ai(),
        "    result = PaymentResult(".ai(),
        "        status='ok',".ai(),
        "        transaction_id=transaction_id,".ai(),
        "        amount=amount,".ai(),
        "    )".ai(),
        "    return {'status': result.status, 'transaction_id': result.transaction_id, 'amount': result.amount}".ai(),
        "    # AI merged: combined validation + result model".ai(),
        "    # end process_payment".ai(),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': models.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["models.py"]);

    // C2': validators.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["validators.py"]);

    // C3': exceptions.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["exceptions.py"]);

    // C4': service.py only (AI-resolved: 24 AI lines in function body)
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["service.py"]);

    // blame at chain[3] for service.py: lines from parent (main's version) are human,
    // all new lines written by AI during conflict resolution are AI.
    assert_blame_at_commit(
        &repo,
        &chain[3],
        "service.py",
        "c4_blame_service",
        &[
            ("def process_payment", false),
            ("validate_amount, validate_card", true),
            ("PaymentError, CardDeclinedError", true),
            ("PaymentResult", true),
            ("import logging", true),
            ("logger = logging", true),
            ("Processing:", true),
            ("if amount <= 0:", false),
            ("must be positive", false),
            ("if not validate_amount", true),
            ("Amount out of range", true),
            ("if not validate_card", true),
            ("Invalid card number", true),
            ("startswith('0000')", true),
            ("CardDeclinedError()", true),
            ("transaction_id = ", true),
            ("Payment OK:", true),
            ("result = PaymentResult(", true),
            ("status='ok',", true),
            ("transaction_id=transaction_id,", true),
            ("amount=amount,", true),
            (")", true),
            ("return {", true),
            ("AI merged", true),
            ("end process_payment", true),
        ],
    );

    // C5': utils.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["utils.py"]);

    human_context.assert_metadata_humans(&repo, &chain[3], "c4'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_complex_function_with_error_handling,
    test_conflict_ai_resolves_complex_function_with_error_handling_standard_human,
);
