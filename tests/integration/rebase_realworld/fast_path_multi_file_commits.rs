use super::{
    ExpectedLineExt, TestRepo, assert_blame_at_commit, assert_blame_sample_at_commit,
    assert_note_base_commit_matches, assert_note_files_exact, assert_note_no_forbidden_files,
    get_commit_chain,
};

#[test]
fn test_fast_path_multi_file_commits_2_files_each() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("manage.py");
    init.set_contents(crate::lines!["# Django-style project management"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits each adding 2 AI files ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: models.py (8 AI lines) + schemas.py (6 AI lines)
    let mut m1 = repo.filename("models.py");
    m1.set_contents(crate::lines![
        "from django.db import models".ai(),
        "".ai(),
        "class Product(models.Model):".ai(),
        "    name = models.CharField(max_length=200)".ai(),
        "    price = models.DecimalField(max_digits=10, decimal_places=2)".ai(),
        "    stock = models.IntegerField(default=0)".ai(),
        "    created_at = models.DateTimeField(auto_now_add=True)".ai(),
        "    class Meta: ordering = ['-created_at']".ai(),
    ]);
    let mut s1 = repo.filename("schemas.py");
    s1.set_contents(crate::lines![
        "from pydantic import BaseModel, condecimal".ai(),
        "from decimal import Decimal".ai(),
        "".ai(),
        "class ProductSchema(BaseModel):".ai(),
        "    name: str".ai(),
        "    price: condecimal(max_digits=10, decimal_places=2)".ai(),
        "    stock: int = 0".ai(),
        "    class Config: from_attributes = True".ai(),
    ]);
    repo.stage_all_and_commit("feat: add models and schemas")
        .unwrap();

    // C2: views.py (8 AI lines) + serializers.py (6 AI lines)
    let mut v2 = repo.filename("views.py");
    v2.set_contents(crate::lines![
        "from django.shortcuts import get_object_or_404".ai(),
        "from rest_framework.decorators import api_view".ai(),
        "from rest_framework.response import Response".ai(),
        "from .models import Product".ai(),
        "from .serializers import ProductSerializer".ai(),
        "".ai(),
        "@api_view(['GET'])".ai(),
        "def product_list(request): return Response(ProductSerializer(Product.objects.all(), many=True).data)".ai(),
    ]);
    let mut sz2 = repo.filename("serializers.py");
    sz2.set_contents(crate::lines![
        "from rest_framework import serializers".ai(),
        "from .models import Product".ai(),
        "".ai(),
        "class ProductSerializer(serializers.ModelSerializer):".ai(),
        "    class Meta:".ai(),
        "        model = Product".ai(),
        "        fields = ['id', 'name', 'price', 'stock', 'created_at']".ai(),
        "        read_only_fields = ['id', 'created_at']".ai(),
    ]);
    repo.stage_all_and_commit("feat: add views and serializers")
        .unwrap();

    // C3: urls.py (6 AI lines) + permissions.py (8 AI lines)
    let mut u3 = repo.filename("urls.py");
    u3.set_contents(crate::lines![
        "from django.urls import path".ai(),
        "from . import views".ai(),
        "".ai(),
        "app_name = 'shop'".ai(),
        "urlpatterns = [".ai(),
        "    path('products/', views.product_list, name='product-list'),".ai(),
        "    path('products/<int:pk>/', views.product_detail, name='product-detail'),".ai(),
        "]".ai(),
    ]);
    let mut p3 = repo.filename("permissions.py");
    p3.set_contents(crate::lines![
        "from rest_framework.permissions import BasePermission".ai(),
        "".ai(),
        "class IsOwnerOrReadOnly(BasePermission):".ai(),
        "    def has_object_permission(self, request, view, obj):".ai(),
        "        if request.method in ('GET', 'HEAD', 'OPTIONS'): return True".ai(),
        "        return obj.owner == request.user".ai(),
        "".ai(),
        "class IsStaff(BasePermission):".ai(),
        "    message = 'Staff access required.'".ai(),
        "    def has_permission(self, request, view): return bool(request.user and request.user.is_staff)".ai(),
    ]);
    repo.stage_all_and_commit("feat: add urls and permissions")
        .unwrap();

    // C4: signals.py (8 AI lines) + tasks.py (6 AI lines)
    let mut sg4 = repo.filename("signals.py");
    sg4.set_contents(crate::lines![
        "from django.db.models.signals import post_save, pre_delete".ai(),
        "from django.dispatch import receiver".ai(),
        "from .models import Product".ai(),
        "".ai(),
        "@receiver(post_save, sender=Product)".ai(),
        "def on_product_saved(sender, instance, created, **kwargs):".ai(),
        "    if created: print(f'New product created: {instance.name}')".ai(),
        "".ai(),
        "@receiver(pre_delete, sender=Product)".ai(),
        "def on_product_deleted(sender, instance, **kwargs):".ai(),
        "    print(f'Deleting product: {instance.name}')".ai(),
    ]);
    let mut t4 = repo.filename("tasks.py");
    t4.set_contents(crate::lines![
        "from celery import shared_task".ai(),
        "from .models import Product".ai(),
        "".ai(),
        "@shared_task".ai(),
        "def sync_inventory(product_id):".ai(),
        "    p = Product.objects.get(id=product_id)".ai(),
        "    # sync with external warehouse system".ai(),
        "    return {'product': p.name, 'stock': p.stock}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add signals and tasks")
        .unwrap();

    // C5: middleware.py (8 AI lines) + decorators.py (6 AI lines)
    let mut mw5 = repo.filename("middleware.py");
    mw5.set_contents(crate::lines![
        "import time".ai(),
        "from django.utils.deprecation import MiddlewareMixin".ai(),
        "".ai(),
        "class RequestTimingMiddleware(MiddlewareMixin):".ai(),
        "    def process_request(self, request):".ai(),
        "        request._start_time = time.monotonic()".ai(),
        "    def process_response(self, request, response):".ai(),
        "        elapsed = (time.monotonic() - getattr(request, '_start_time', time.monotonic())) * 1000".ai(),
        "        response['X-Response-Time'] = f'{elapsed:.1f}ms'".ai(),
        "        return response".ai(),
    ]);
    let mut d5 = repo.filename("decorators.py");
    d5.set_contents(crate::lines![
        "from functools import wraps".ai(),
        "from django.http import JsonResponse".ai(),
        "".ai(),
        "def require_json(view_fn):".ai(),
        "    @wraps(view_fn)".ai(),
        "    def wrapper(request, *a, **kw):".ai(),
        "        if request.content_type != 'application/json': return JsonResponse({'error': 'JSON required'}, status=415)".ai(),
        "        return view_fn(request, *a, **kw)".ai(),
        "    return wrapper".ai(),
    ]);
    repo.stage_all_and_commit("feat: add middleware and decorators")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "settings.py",
        "DEBUG = True\nINSTALLED_APPS = ['django.contrib.admin']\n",
        "config: add Django settings",
    );
    repo.commit_untracked_file(
        "requirements.txt",
        "django==4.2\ndjangorestframework==3.14\ncelery==5.3\npydantic==2.0\n",
        "deps: add requirements.txt",
    );
    repo.commit_untracked_file("Dockerfile",
        "FROM python:3.11\nWORKDIR /app\nCOPY requirements.txt .\nRUN pip install -r requirements.txt\n",
        "build: add Dockerfile",
    );
    repo.commit_untracked_file("docker-compose.yml",
        "version: '3.9'\nservices:\n  web:\n    build: .\n    ports: ['8000:8000']\n  worker:\n    build: .\n    command: celery -A app worker\n",
        "build: add docker-compose.yml",
    );
    repo.commit_untracked_file(
        ".env.example",
        "SECRET_KEY=change-me\nDEBUG=1\nDATABASE_URL=postgresql://localhost/app\n",
        "config: add .env.example",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {models.py, schemas.py}
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["models.py", "schemas.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "views.py",
            "serializers.py",
            "urls.py",
            "permissions.py",
            "signals.py",
            "tasks.py",
            "middleware.py",
            "decorators.py",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "models.py",
        "sha0_blame_models",
        &[
            ("from django.db import models", true),
            ("", true),
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
            ("price = models.DecimalField", true),
            ("stock = models.IntegerField", true),
            ("created_at = models.DateTimeField", true),
            ("class Meta: ordering", true),
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "schemas.py",
        "sha0_blame_schemas",
        &[
            ("from pydantic import BaseModel, condecimal", true),
            ("from decimal import Decimal", true),
            ("", true),
            ("class ProductSchema(BaseModel):", true),
            ("name: str", true),
            ("price: condecimal", true),
            ("stock: int = 0", true),
            ("class Config: from_attributes = True", true),
        ],
    );

    // sha1 = C2': views.py + serializers.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(
        &repo,
        &chain[1],
        "sha1_files",
        &["views.py", "serializers.py"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &[
            "urls.py",
            "permissions.py",
            "signals.py",
            "tasks.py",
            "middleware.py",
            "decorators.py",
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "models.py",
        "chain1_prior_models.py",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "schemas.py",
        "chain1_prior_schemas.py",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
        ],
    );

    // sha2 = C3': urls.py + permissions.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(
        &repo,
        &chain[2],
        "sha2_files",
        &["urls.py", "permissions.py"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["signals.py", "tasks.py", "middleware.py", "decorators.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "models.py",
        "chain2_prior_models.py",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "schemas.py",
        "chain2_prior_schemas.py",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "views.py",
        "chain2_prior_views.py",
        &[
            ("@api_view(['GET'])", true),
            ("def product_list(request):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "serializers.py",
        "chain2_prior_serializers.py",
        &[
            (
                "class ProductSerializer(serializers.ModelSerializer):",
                true,
            ),
            ("model = Product", true),
        ],
    );

    // sha3 = C4': signals.py + tasks.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["signals.py", "tasks.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[3],
        "sha3_no_future",
        &["middleware.py", "decorators.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "models.py",
        "chain3_prior_models.py",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "schemas.py",
        "chain3_prior_schemas.py",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "views.py",
        "chain3_prior_views.py",
        &[
            ("@api_view(['GET'])", true),
            ("def product_list(request):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "serializers.py",
        "chain3_prior_serializers.py",
        &[
            (
                "class ProductSerializer(serializers.ModelSerializer):",
                true,
            ),
            ("model = Product", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "urls.py",
        "chain3_prior_urls.py",
        &[("app_name = 'shop'", true), ("urlpatterns = [", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "permissions.py",
        "chain3_prior_permissions.py",
        &[
            ("class IsOwnerOrReadOnly(BasePermission):", true),
            ("class IsStaff(BasePermission):", true),
        ],
    );

    // sha4 = C5': middleware.py + decorators.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["middleware.py", "decorators.py"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "middleware.py",
        "sha4_blame_mw",
        &[
            ("import time", true),
            ("from django.utils.deprecation import MiddlewareMixin", true),
            ("", true),
            ("class RequestTimingMiddleware(MiddlewareMixin):", true),
            ("def process_request(self, request):", true),
            ("request._start_time = time.monotonic()", true),
            ("def process_response(self, request, response):", true),
            ("elapsed = (time.monotonic()", true),
            ("response['X-Response-Time']", true),
            ("return response", true),
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "decorators.py",
        "sha4_blame_dec",
        &[
            ("from functools import wraps", true),
            ("from django.http import JsonResponse", true),
            ("", true),
            ("def require_json(view_fn):", true),
            ("@wraps(view_fn)", true),
            ("def wrapper(request, *a, **kw):", true),
            ("if request.content_type", true),
            ("return view_fn(request", true),
            ("return wrapper", true),
        ],
    );
    // Verify C1's files (models.py and schemas.py) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "models.py",
        "sha4_models_preserved",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
            ("class Meta: ordering", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "schemas.py",
        "sha4_schemas_preserved",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
            ("class Config: from_attributes = True", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "views.py",
        "chain4_prior_views.py",
        &[
            ("@api_view(['GET'])", true),
            ("def product_list(request):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "serializers.py",
        "chain4_prior_serializers.py",
        &[
            (
                "class ProductSerializer(serializers.ModelSerializer):",
                true,
            ),
            ("model = Product", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "urls.py",
        "chain4_prior_urls.py",
        &[("app_name = 'shop'", true), ("urlpatterns = [", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "permissions.py",
        "chain4_prior_permissions.py",
        &[
            ("class IsOwnerOrReadOnly(BasePermission):", true),
            ("class IsStaff(BasePermission):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "signals.py",
        "chain4_prior_signals.py",
        &[
            ("@receiver(post_save, sender=Product)", true),
            ("@receiver(pre_delete, sender=Product)", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "tasks.py",
        "chain4_prior_tasks.py",
        &[
            ("@shared_task", true),
            ("def sync_inventory(product_id):", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_fast_path_multi_file_commits_2_files_each,);
