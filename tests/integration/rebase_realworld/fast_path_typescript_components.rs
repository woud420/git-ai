use super::*;

const PRIOR_SAMPLES: &[PriorBlameSample] = &[
    (
        "Button.tsx",
        "button_tsx",
        ["interface ButtonProps {", "export function Button"],
    ),
    (
        "Input.tsx",
        "input_tsx",
        ["interface InputProps {", "export function Input"],
    ),
    (
        "Modal.tsx",
        "modal_tsx",
        ["interface ModalProps {", "export function Modal"],
    ),
    (
        "Table.tsx",
        "table_tsx",
        ["interface TableProps<T> {", "export function Table<T"],
    ),
];

#[test]
fn test_fast_path_typescript_frontend_5_components() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("src/index.ts");
    init.set_contents(crate::lines!["// TypeScript frontend entry point"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits, each adding a React component ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: Button.tsx
    let mut f1 = repo.filename("Button.tsx");
    f1.set_contents(crate::lines![
        "interface ButtonProps {".ai(),
        "  label: string;".ai(),
        "  onClick: () => void;".ai(),
        "  disabled?: boolean;".ai(),
        "  variant?: 'primary' | 'secondary' | 'danger';".ai(),
        "}".ai(),
        "export function Button({ label, onClick, disabled = false, variant = 'primary' }: ButtonProps) {".ai(),
        "  const cls = `btn btn-${variant}${disabled ? ' btn-disabled' : ''}`;".ai(),
        "  return <button className={cls} onClick={onClick} disabled={disabled}>{label}</button>;".ai(),
        "}".ai(),
        "export default Button;".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Button component")
        .unwrap();

    // C2: Input.tsx
    let mut f2 = repo.filename("Input.tsx");
    f2.set_contents(crate::lines![
        "interface InputProps {".ai(),
        "  value: string;".ai(),
        "  onChange: (v: string) => void;".ai(),
        "  placeholder?: string;".ai(),
        "  type?: 'text' | 'email' | 'password';".ai(),
        "  error?: string;".ai(),
        "}".ai(),
        "export function Input({ value, onChange, placeholder, type = 'text', error }: InputProps) {".ai(),
        "  return (".ai(),
        "    <div className=\"input-wrapper\">".ai(),
        "      <input type={type} value={value} placeholder={placeholder} onChange={e => onChange(e.target.value)} />".ai(),
        "      {error && <span className=\"input-error\">{error}</span>}".ai(),
        "    </div>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Input component")
        .unwrap();

    // C3: Modal.tsx
    let mut f3 = repo.filename("Modal.tsx");
    f3.set_contents(crate::lines![
        "interface ModalProps {".ai(),
        "  isOpen: boolean;".ai(),
        "  onClose: () => void;".ai(),
        "  title: string;".ai(),
        "  children: React.ReactNode;".ai(),
        "}".ai(),
        "export function Modal({ isOpen, onClose, title, children }: ModalProps) {".ai(),
        "  if (!isOpen) return null;".ai(),
        "  return (".ai(),
        "    <div className=\"modal-overlay\" onClick={onClose}>".ai(),
        "      <div className=\"modal-content\" onClick={e => e.stopPropagation()}>".ai(),
        "        <h2>{title}</h2>".ai(),
        "        <div className=\"modal-body\">{children}</div>".ai(),
        "      </div>".ai(),
        "    </div>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Modal component")
        .unwrap();

    // C4: Table.tsx
    let mut f4 = repo.filename("Table.tsx");
    f4.set_contents(crate::lines![
        "interface Column<T> { key: keyof T; header: string; }".ai(),
        "interface TableProps<T> {".ai(),
        "  columns: Column<T>[];".ai(),
        "  data: T[];".ai(),
        "  onRowClick?: (row: T) => void;".ai(),
        "}".ai(),
        "export function Table<T extends { id: string | number }>({ columns, data, onRowClick }: TableProps<T>) {".ai(),
        "  return (".ai(),
        "    <table className=\"data-table\">".ai(),
        "      <thead><tr>{columns.map(c => <th key={String(c.key)}>{c.header}</th>)}</tr></thead>".ai(),
        "      <tbody>{data.map(row => <tr key={row.id} onClick={() => onRowClick?.(row)}>{columns.map(c => <td key={String(c.key)}>{String(row[c.key])}</td>)}</tr>)}</tbody>".ai(),
        "    </table>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Table component")
        .unwrap();

    // C5: Form.tsx
    let mut f5 = repo.filename("Form.tsx");
    f5.set_contents(crate::lines![
        "interface FormField { name: string; label: string; type: string; required?: boolean; }".ai(),
        "interface FormProps {".ai(),
        "  fields: FormField[];".ai(),
        "  onSubmit: (data: Record<string, string>) => void;".ai(),
        "  submitLabel?: string;".ai(),
        "}".ai(),
        "export function Form({ fields, onSubmit, submitLabel = 'Submit' }: FormProps) {".ai(),
        "  const [values, setValues] = React.useState<Record<string, string>>({});".ai(),
        "  const handleSubmit = (e: React.FormEvent) => { e.preventDefault(); onSubmit(values); };".ai(),
        "  return (".ai(),
        "    <form onSubmit={handleSubmit}>".ai(),
        "      {fields.map(f => <label key={f.name}>{f.label}<input name={f.name} type={f.type} required={f.required} onChange={e => setValues(v => ({...v, [f.name]: e.target.value}))} /></label>)}".ai(),
        "      <button type=\"submit\">{submitLabel}</button>".ai(),
        "    </form>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Form component")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on config files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "vite.config.ts",
        "import { defineConfig } from 'vite';\nexport default defineConfig({ plugins: [] });\n",
        "build: add vite config",
    );
    repo.commit_untracked_file(
        ".eslintrc.json",
        "{\"extends\": [\"eslint:recommended\", \"plugin:@typescript-eslint/recommended\"]}\n",
        "lint: add eslint config",
    );
    repo.commit_untracked_file("tsconfig.json",
        "{\"compilerOptions\": {\"target\": \"ES2020\", \"module\": \"ESNext\", \"jsx\": \"react-jsx\", \"strict\": true}}\n",
        "build: add tsconfig",
    );
    repo.commit_untracked_file("package.json",
        "{\"name\": \"frontend\", \"version\": \"1.0.0\", \"scripts\": {\"dev\": \"vite\", \"build\": \"vite build\"}}\n",
        "build: add package.json",
    );
    repo.commit_untracked_file("tailwind.config.js",
        "module.exports = { content: ['./src/**/*.{ts,tsx}'], theme: { extend: {} }, plugins: [] };\n",
        "style: add tailwind config",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': only Button.tsx
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["Button.tsx"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &["Input.tsx", "Modal.tsx", "Table.tsx", "Form.tsx"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "Button.tsx",
        "sha0_blame",
        &[
            ("interface ButtonProps {", true),
            ("label: string;", true),
            ("onClick: () => void;", true),
            ("disabled?: boolean;", true),
            ("variant?: 'primary' | 'secondary' | 'danger';", true),
            ("}", true),
            ("export function Button", true),
            ("const cls =", true),
            ("return <button", true),
            ("}", true),
            ("export default Button;", true),
        ],
    );

    // sha1 = C2': Input.tsx
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["Input.tsx"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["Modal.tsx", "Table.tsx", "Form.tsx"],
    );
    assert_prior_blame_samples(&repo, &chain[1], 1, &PRIOR_SAMPLES[0..1]);

    // sha2 = C3': Modal.tsx
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["Modal.tsx"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["Table.tsx", "Form.tsx"],
    );
    assert_prior_blame_samples(&repo, &chain[2], 2, &PRIOR_SAMPLES[0..2]);

    // sha3 = C4': Table.tsx
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["Table.tsx"]);
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["Form.tsx"]);
    assert_prior_blame_samples(&repo, &chain[3], 3, &PRIOR_SAMPLES[0..3]);

    // sha4 = C5': Form.tsx
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["Form.tsx"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "Form.tsx",
        "sha4_blame",
        &[
            ("interface FormField", true),
            ("interface FormProps {", true),
            ("fields: FormField[];", true),
            ("onSubmit: (data: Record<string, string>) => void;", true),
            ("submitLabel?: string;", true),
            ("}", true),
            ("export function Form", true),
            ("const [values, setValues]", true),
            ("const handleSubmit", true),
            ("return (", true),
            ("<form onSubmit={handleSubmit}>", true),
            ("fields.map", true),
            ("<button type=\"submit\">", true),
            ("</form>", true),
            (");", true),
            ("}", true),
        ],
    );
    // Verify C1's file (Button.tsx) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "Button.tsx",
        "sha4_button_preserved",
        &[
            ("interface ButtonProps {", true),
            ("export function Button", true),
            ("export default Button;", true),
        ],
    );
    assert_prior_blame_samples(&repo, &chain[4], 4, &PRIOR_SAMPLES[1..4]);
}

crate::reuse_tests_in_worktree!(test_fast_path_typescript_frontend_5_components,);
