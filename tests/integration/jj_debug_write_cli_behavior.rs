use super::*;

pub fn initialize_case(case: &Case, config: &Config) {
    fs::write(
        case.root.join("dirty-write-cli.txt"),
        b"unchanged dirty bytes\n",
    )
    .unwrap();
    let (journal, saved) = initialize_cli(case, config);
    let baseline = saved.baseline().receipt().clone();
    let zero = status(case, &journal, config).cursor().clone();
    let a = native::rich_parent();
    select(
        case,
        std::slice::from_ref(&a),
        &[&a.operation_id],
        Some(&a.operation_id),
    );
    let (_, _, cursor) = capture_cli(case, config, &zero);
    assert_eq!(cursor.generation(), 1);
    let before = total_state(case);
    let result = invoke(case, &initialize_args(&case.journal_path));
    let reopened = case.reopen(&journal, config).unwrap().unwrap();
    assert_eq!(result, registration_json(&reopened, "already_registered"));
    assert_eq!(reopened.baseline().receipt(), &baseline);
    assert_eq!(reopened.attachment_id(), saved.attachment_id());
    assert_eq!(
        reopened.checkout_relation(),
        JjRegisteredCheckoutRelation::OutsideBaseline
    );
    assert!(total_state(case) == before);
    assert_eq!(
        fs::read(case.root.join("dirty-write-cli.txt")).unwrap(),
        b"unchanged dirty bytes\n"
    );
}

pub fn capture_case(case: &Case, config: &Config) {
    let (journal, _) = initialize_cli(case, config);
    let zero = status(case, &journal, config).cursor().clone();
    let (_, empty, one) = capture_cli(case, config, &zero);
    assert!(empty.ordered_operations().is_empty());
    assert_eq!(one.generation(), 1);
    assert_eq!(
        checked(
            case,
            command(
                case,
                &expect_args(&case.journal_path, zero.expectation()),
                &case.root
            ),
            None
        ),
        capture_json(&empty, &one, "already_admitted")
    );
    let a = native::rich_parent();
    let b = native::rich_child();
    select(
        case,
        &[a.clone(), b.clone()],
        &[&a.operation_id],
        Some(&a.operation_id),
    );
    let (_, first, two) = capture_cli(case, config, &one);
    assert_eq!(first.ordered_operations(), std::slice::from_ref(&a));
    case.heads(&[&b.operation_id]);
    case.write_checkout(&b.operation_id);
    checked(
        case,
        command(
            case,
            &expect_args(&case.journal_path, one.expectation()),
            &case.root,
        ),
        Some("admission_unavailable"),
    );
    for field in [
        "--expect-source",
        "--expect-initialization-receipt",
        "--expect-baseline",
    ] {
        let mut args = expect_args(&case.journal_path, two.expectation());
        let index = args.iter().position(|arg| arg == field).unwrap();
        args[index + 1] = "0".repeat(64).into();
        checked(
            case,
            command(case, &args, &case.root),
            Some("admission_unavailable"),
        );
    }
    let (_, second, three) = capture_cli(case, config, &two);
    assert_eq!(second.ordered_operations(), &[a.clone(), b]);
    assert_eq!(three.generation(), 3);
    case.heads(&[&a.operation_id]);
    case.write_checkout(&a.operation_id);
    assert_eq!(
        checked(
            case,
            command(
                case,
                &expect_args(&case.journal_path, one.expectation()),
                &case.root
            ),
            None
        ),
        capture_json(&first, &three, "already_admitted")
    );
    let (_, repeated, four) = capture_cli(case, config, &three);
    assert_eq!(repeated.ordered_operations(), first.ordered_operations());
    assert_ne!(
        repeated.receipt().admission_id(),
        first.receipt().admission_id()
    );
    assert_eq!(four.generation(), 4);
}

pub fn head_order(case: &Case, config: &Config) {
    let a = native::rich_parent();
    case.write_evidence(&a);
    case.heads(&[MERGE_ID, &a.operation_id]);
    let (journal, _) = initialize_cli(case, config);
    let zero = status(case, &journal, config).cursor().clone();
    assert_eq!(zero.admitted_head_ids().len(), 2);
    let (_, saved, current) = capture_cli(case, config, &zero);
    assert!(saved.ordered_operations().is_empty());
    let mut heads = zero.admitted_head_ids().to_vec();
    heads.reverse();
    let mut expected = zero.expectation();
    expected.admitted_head_ids = &heads;
    let args = expect_args(&case.journal_path, expected);
    assert_eq!(
        checked(case, command(case, &args, &case.root), None),
        capture_json(&saved, &current, "already_admitted")
    );
}
