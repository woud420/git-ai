use super::{
    ExpectedLineExt, TestRepo, assert_blame_sample_at_commit, assert_note_base_commit_matches,
    assert_note_files_exact, get_commit_chain,
};

/// Test 6: Two shared files (models.py + services.py), both prepended by main.
/// Feature appends AI lines to both in each commit. Checks cumulative lines
/// across both files and no future-file leak.
#[test]
fn test_slow_path_multiple_shared_files_both_modified() {
    let repo = TestRepo::new();

    // Initial: both shared files with trailing newline
    repo.commit_untracked_file(
        "models.py",
        "class BaseModel: pass\n",
        "Initial commit: models.py",
    );
    repo.commit_untracked_file(
        "services.py",
        "class BaseService: pass\n",
        "Initial commit: services.py",
    );
    let main_branch = repo.current_branch();

    // Main: prepend headers to BOTH files (two separate commits, then 3 more human commits)
    repo.commit_untracked_file(
        "models.py",
        "# Domain models\nfrom dataclasses import dataclass\n\nclass BaseModel: pass\n",
        "main: prepend header to models.py",
    );
    repo.commit_untracked_file(
        "services.py",
        "# Business services\nfrom typing import Any\n\nclass BaseService: pass\n",
        "main: prepend header to services.py",
    );
    repo.commit_untracked_file(
        "exceptions.py",
        "class NotFound(Exception): pass\nclass Conflict(Exception): pass\n",
        "main: add exceptions",
    );
    repo.commit_untracked_file("validators.py",
        "def validate_not_empty(val, name):\n    if not val: raise ValueError(f'{name} must not be empty')\n",
        "main: add validators",
    );
    repo.commit_untracked_file(
        "constants.py",
        "DEFAULT_PAGE_SIZE = 20\nMAX_PAGE_SIZE = 100\n",
        "main: add constants",
    );

    // Feature branch from before main's two prepend commits (HEAD~5)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 6 AI lines to BOTH models.py AND services.py (12 total)
    let mut models = repo.filename("models.py");
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User:".ai(),
        "    id: int".ai(),
        "    email: str".ai(),
        "    name: str".ai(),
        "    active: bool = True".ai(),
    ]);
    let mut services = repo.filename("services.py");
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_id(self, user_id: int): return self.repo.find(user_id)".ai(),
        "    def list_active(self): return self.repo.find_all(active=True)".ai(),
        "    def deactivate(self, user_id: int): self.repo.update(user_id, active=False)".ai(),
        "    def exists(self, email: str) -> bool: return self.repo.find_by_email(email) is not None".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add User model + UserService")
        .unwrap();

    // C2: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User:".ai(),
        "    id: int".ai(),
        "    email: str".ai(),
        "    name: str".ai(),
        "    active: bool = True".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Product:".ai(),
        "    id: int".ai(),
        "    name: str".ai(),
        "    price: float".ai(),
        "    stock: int = 0".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_id(self, user_id: int): return self.repo.find(user_id)".ai(),
        "    def list_active(self): return self.repo.find_all(active=True)".ai(),
        "".ai(),
        "class ProductService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_id(self, pid: int): return self.repo.find(pid)".ai(),
        "    def list_in_stock(self): return self.repo.find_all(stock__gt=0)".ai(),
        "    def adjust_stock(self, pid: int, delta: int): self.repo.increment(pid, 'stock', delta)".ai(),
        "    def get_price(self, pid: int) -> float: return self.repo.find(pid).price".ai(),
        "    def set_price(self, pid: int, price: float): self.repo.update(pid, price=price)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Product model + ProductService")
        .unwrap();

    // C3: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User: id: int; email: str; name: str; active: bool = True".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Product: id: int; name: str; price: float; stock: int = 0".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Order:".ai(),
        "    id: int".ai(),
        "    user_id: int".ai(),
        "    items: list".ai(),
        "    total: float".ai(),
        "    status: str = 'pending'".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService:".ai(),
        "    def get_by_id(self, user_id: int): return self.repo.find(user_id)".ai(),
        "".ai(),
        "class ProductService:".ai(),
        "    def get_by_id(self, pid: int): return self.repo.find(pid)".ai(),
        "    def list_in_stock(self): return self.repo.find_all(stock__gt=0)".ai(),
        "".ai(),
        "class OrderService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def create(self, user_id, items): return self.repo.create(user_id=user_id, items=items)".ai(),
        "    def get_by_id(self, oid: int): return self.repo.find(oid)".ai(),
        "    def cancel(self, oid: int): self.repo.update(oid, status='cancelled')".ai(),
        "    def complete(self, oid: int): self.repo.update(oid, status='completed')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Order model + OrderService")
        .unwrap();

    // C4: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User: id: int; email: str; name: str".ai(),
        "@dataclass".ai(),
        "class Product: id: int; name: str; price: float; stock: int = 0".ai(),
        "@dataclass".ai(),
        "class Order: id: int; user_id: int; items: list; total: float; status: str = 'pending'"
            .ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Address:".ai(),
        "    id: int".ai(),
        "    user_id: int".ai(),
        "    street: str".ai(),
        "    city: str".ai(),
        "    country: str = 'US'".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService: pass".ai(),
        "class ProductService: pass".ai(),
        "class OrderService: pass".ai(),
        "".ai(),
        "class AddressService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_user(self, uid: int): return self.repo.find_all(user_id=uid)".ai(),
        "    def create(self, uid, street, city, country='US'): return self.repo.create(user_id=uid, street=street, city=city, country=country)".ai(),
        "    def delete(self, aid: int): self.repo.delete(aid)".ai(),
        "    def set_default(self, uid: int, aid: int): self.repo.update_all({'is_default': False}, user_id=uid); self.repo.update(aid, is_default=True)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Address model + AddressService")
        .unwrap();

    // C5: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User: id: int; email: str; name: str".ai(),
        "@dataclass".ai(),
        "class Product: id: int; name: str; price: float; stock: int = 0".ai(),
        "@dataclass".ai(),
        "class Order: id: int; user_id: int; items: list; total: float; status: str = 'pending'"
            .ai(),
        "@dataclass".ai(),
        "class Address: id: int; user_id: int; street: str; city: str; country: str = 'US'".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Review:".ai(),
        "    id: int".ai(),
        "    user_id: int".ai(),
        "    product_id: int".ai(),
        "    rating: int".ai(),
        "    comment: str = ''".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService: pass".ai(),
        "class ProductService: pass".ai(),
        "class OrderService: pass".ai(),
        "class AddressService: pass".ai(),
        "".ai(),
        "class ReviewService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def create(self, uid, pid, rating, comment=''): return self.repo.create(user_id=uid, product_id=pid, rating=rating, comment=comment)".ai(),
        "    def get_for_product(self, pid: int): return self.repo.find_all(product_id=pid)".ai(),
        "    def average_rating(self, pid: int) -> float: reviews = self.get_for_product(pid); return sum(r.rating for r in reviews) / len(reviews) if reviews else 0.0".ai(),
        "    def delete(self, rid: int): self.repo.delete(rid)".ai(),
        "    def update_comment(self, rid: int, comment: str): self.repo.update(rid, comment=comment)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add Review model + ReviewService")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {models.py, services.py} ~12 accepted lines
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(
        &repo,
        &chain[0],
        "sha0_files",
        &["models.py", "services.py"],
    );
    // sha1 = C2': {models.py, services.py} ~12 accepted lines (only C2's delta)
    // C2 added Product model to models.py and ProductService to services.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(
        &repo,
        &chain[1],
        "sha1_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "models.py",
        "sha1_models_product",
        &[("class Product:", true), ("price: float", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "services.py",
        "sha1_services_product",
        &[("class ProductService:", true), ("def list_in_stock", true)],
    );

    // sha2 = C3': {models.py, services.py} ~12 accepted lines (only C3's delta)
    // C3 added Order model to models.py and OrderService to services.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(
        &repo,
        &chain[2],
        "sha2_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "models.py",
        "sha2_models_order",
        &[("class Order:", true), ("status: str = 'pending'", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "services.py",
        "sha2_services_order",
        &[("class OrderService:", true), ("def cancel", true)],
    );

    // sha3 = C4': ~12 accepted lines (only C4's delta)
    // C4 added Address model to models.py and AddressService to services.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(
        &repo,
        &chain[3],
        "sha3_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "models.py",
        "sha3_models_address",
        &[("class Address:", true), ("country: str = 'US'", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "services.py",
        "sha3_services_address",
        &[("class AddressService:", true), ("def get_by_user", true)],
    );

    // sha4 = C5': ~12 accepted lines (only C5's delta)
    // C5 added Review model to models.py and ReviewService to services.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "models.py",
        "sha4_models_review",
        &[("class Review:", true), ("rating: int", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "services.py",
        "sha4_services_review",
        &[("class ReviewService:", true), ("def average_rating", true)],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_multiple_shared_files_both_modified,);
