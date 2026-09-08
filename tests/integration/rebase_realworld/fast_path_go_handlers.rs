use super::{
    ExpectedLineExt, TestRepo, assert_blame_at_commit, assert_blame_sample_at_commit,
    assert_note_base_commit_matches, assert_note_files_exact, assert_note_no_forbidden_files,
    get_commit_chain,
};

#[test]
fn test_fast_path_go_service_5_handlers() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("main.go");
    init.set_contents(crate::lines!["// Go HTTP service"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits, each adding a Go handler file ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: handlers/user.go
    let mut f1 = repo.filename("handlers/user.go");
    f1.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import \"net/http\"".ai(),
        "".ai(),
        "type UserHandler struct { store UserStore }".ai(),
        "".ai(),
        "func NewUserHandler(s UserStore) *UserHandler { return &UserHandler{store: s} }".ai(),
        "".ai(),
        "func (h *UserHandler) GetUser(w http.ResponseWriter, r *http.Request) {".ai(),
        "    id := r.PathValue(\"id\")".ai(),
        "    user, err := h.store.Find(id)".ai(),
        "    if err != nil { http.Error(w, err.Error(), http.StatusNotFound); return }".ai(),
        "    writeJSON(w, user)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add user handler").unwrap();

    // C2: handlers/product.go
    let mut f2 = repo.filename("handlers/product.go");
    f2.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import \"net/http\"".ai(),
        "".ai(),
        "type ProductHandler struct { store ProductStore }".ai(),
        "".ai(),
        "func NewProductHandler(s ProductStore) *ProductHandler { return &ProductHandler{store: s} }".ai(),
        "".ai(),
        "func (h *ProductHandler) ListProducts(w http.ResponseWriter, r *http.Request) {".ai(),
        "    products, err := h.store.List()".ai(),
        "    if err != nil { http.Error(w, err.Error(), http.StatusInternalServerError); return }".ai(),
        "    writeJSON(w, products)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add product handler")
        .unwrap();

    // C3: handlers/order.go
    let mut f3 = repo.filename("handlers/order.go");
    f3.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import (\"net/http\"; \"encoding/json\")".ai(),
        "".ai(),
        "type OrderHandler struct { store OrderStore }".ai(),
        "".ai(),
        "func NewOrderHandler(s OrderStore) *OrderHandler { return &OrderHandler{store: s} }".ai(),
        "".ai(),
        "func (h *OrderHandler) CreateOrder(w http.ResponseWriter, r *http.Request) {".ai(),
        "    var req CreateOrderRequest".ai(),
        "    if err := json.NewDecoder(r.Body).Decode(&req); err != nil { http.Error(w, err.Error(), http.StatusBadRequest); return }".ai(),
        "    order, err := h.store.Create(req)".ai(),
        "    if err != nil { http.Error(w, err.Error(), http.StatusInternalServerError); return }".ai(),
        "    writeJSON(w, order)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add order handler")
        .unwrap();

    // C4: handlers/auth.go
    let mut f4 = repo.filename("handlers/auth.go");
    f4.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import (\"net/http\"; \"time\")".ai(),
        "".ai(),
        "type AuthHandler struct { svc AuthService }".ai(),
        "".ai(),
        "func NewAuthHandler(s AuthService) *AuthHandler { return &AuthHandler{svc: s} }".ai(),
        "".ai(),
        "func (h *AuthHandler) Login(w http.ResponseWriter, r *http.Request) {".ai(),
        "    token, err := h.svc.Authenticate(r.FormValue(\"user\"), r.FormValue(\"pass\"))".ai(),
        "    if err != nil { http.Error(w, \"unauthorized\", http.StatusUnauthorized); return }".ai(),
        "    http.SetCookie(w, &http.Cookie{Name: \"session\", Value: token, Expires: time.Now().Add(24*time.Hour)})".ai(),
        "    w.WriteHeader(http.StatusOK)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add auth handler").unwrap();

    // C5: handlers/health.go
    let mut f5 = repo.filename("handlers/health.go");
    f5.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import (\"net/http\"; \"encoding/json\")".ai(),
        "".ai(),
        "type HealthHandler struct { version string }".ai(),
        "".ai(),
        "func NewHealthHandler(v string) *HealthHandler { return &HealthHandler{version: v} }".ai(),
        "".ai(),
        "func (h *HealthHandler) Health(w http.ResponseWriter, r *http.Request) {".ai(),
        "    json.NewEncoder(w).Encode(map[string]string{\"status\": \"ok\", \"version\": h.version})".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add health handler")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "go.mod",
        "module example.com/service\n\ngo 1.21\n",
        "build: add go.mod",
    );
    repo.commit_untracked_file(
        "cmd/main.go",
        "package main\n\nfunc main() {}\n",
        "build: add cmd/main.go",
    );
    repo.commit_untracked_file("Dockerfile",
        "FROM golang:1.21\nWORKDIR /app\nCOPY . .\nRUN go build -o server cmd/main.go\nCMD [\"./server\"]\n",
        "build: add Dockerfile",
    );
    repo.commit_untracked_file(
        "docker-compose.yml",
        "version: '3.8'\nservices:\n  app:\n    build: .\n    ports:\n      - '8080:8080'\n",
        "build: add docker-compose.yml",
    );
    repo.commit_untracked_file(
        "Makefile",
        "build:\n\tgo build ./...\ntest:\n\tgo test ./...\n.PHONY: build test\n",
        "build: add Makefile",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': only handlers/user.go
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["handlers/user.go"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "handlers/product.go",
            "handlers/order.go",
            "handlers/auth.go",
            "handlers/health.go",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "handlers/user.go",
        "sha0_blame",
        &[
            ("package handlers", true),
            ("", true),
            ("import \"net/http\"", true),
            ("", true),
            ("type UserHandler struct", true),
            ("", true),
            ("func NewUserHandler", true),
            ("", true),
            ("func (h *UserHandler) GetUser", true),
            ("id := r.PathValue", true),
            ("user, err := h.store.Find", true),
            ("if err != nil", true),
            ("writeJSON(w, user)", true),
            ("}", true),
        ],
    );

    // sha1 = C2': product
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["handlers/product.go"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &[
            "handlers/order.go",
            "handlers/auth.go",
            "handlers/health.go",
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "handlers/user.go",
        "chain1_prior_user_go",
        &[
            ("type UserHandler struct", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );

    // sha2 = C3': order
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["handlers/order.go"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["handlers/auth.go", "handlers/health.go"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "handlers/user.go",
        "chain2_prior_user_go",
        &[
            ("type UserHandler struct", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "handlers/product.go",
        "chain2_prior_product_go",
        &[
            ("type ProductHandler struct", true),
            ("func (h *ProductHandler) ListProducts", true),
        ],
    );

    // sha3 = C4': auth
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["handlers/auth.go"]);
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["handlers/health.go"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "handlers/user.go",
        "chain3_prior_user_go",
        &[
            ("type UserHandler struct", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "handlers/product.go",
        "chain3_prior_product_go",
        &[
            ("type ProductHandler struct", true),
            ("func (h *ProductHandler) ListProducts", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "handlers/order.go",
        "chain3_prior_order_go",
        &[
            ("type OrderHandler struct", true),
            ("func (h *OrderHandler) CreateOrder", true),
        ],
    );

    // sha4 = C5': health
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["handlers/health.go"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "handlers/health.go",
        "sha4_blame",
        &[
            ("package handlers", true),
            ("", true),
            ("import", true),
            ("", true),
            ("type HealthHandler struct", true),
            ("", true),
            ("func NewHealthHandler", true),
            ("", true),
            ("func (h *HealthHandler) Health", true),
            ("json.NewEncoder(w).Encode", true),
            ("}", true),
        ],
    );
    // Verify C1's file (handlers/user.go) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/user.go",
        "sha4_user_preserved",
        &[
            ("type UserHandler struct", true),
            ("func NewUserHandler", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/product.go",
        "chain4_prior_product_go",
        &[
            ("type ProductHandler struct", true),
            ("func (h *ProductHandler) ListProducts", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/order.go",
        "chain4_prior_order_go",
        &[
            ("type OrderHandler struct", true),
            ("func (h *OrderHandler) CreateOrder", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/auth.go",
        "chain4_prior_auth_go",
        &[
            ("type AuthHandler struct", true),
            ("func (h *AuthHandler) Login", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_fast_path_go_service_5_handlers,);
