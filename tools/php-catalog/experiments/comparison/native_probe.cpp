// Bounded representation experiment, not a catalog generator or a sound solver.
#include <clang/AST/ASTConsumer.h>
#include <clang/AST/RecordLayout.h>
#include <clang/AST/RecursiveASTVisitor.h>
#include <clang/Analysis/CFG.h>
#include <clang/Analysis/FlowSensitive/NoopAnalysis.h>
#include <clang/Frontend/FrontendActions.h>
#include <clang/Tooling/CompilationDatabase.h>
#include <clang/Tooling/Tooling.h>
#include <llvm/IR/Instructions.h>
#include <llvm/IR/Module.h>
#include <llvm/IR/ModuleSlotTracker.h>
#include <llvm/IRReader/IRReader.h>
#include <llvm/Support/JSON.h>
#include <llvm/Support/SourceMgr.h>
#include <llvm/Support/raw_ostream.h>
#include <set>
#include <string>

using namespace clang;
namespace j = llvm::json;
static std::set<std::string> entries;
static int flowBudget = 0;
static bool selected(llvm::StringRef name) {
  return entries.empty() || entries.count(name.str());
}
static std::string text(const Stmt *s, ASTContext &ctx) {
  if (!s) return "";
  std::string result;
  llvm::raw_string_ostream stream(result);
  s->printPretty(stream, nullptr, ctx.getPrintingPolicy());
  return result;
}
class Visitor : public RecursiveASTVisitor<Visitor> {
  ASTContext &ctx;
public:
  j::Array functions, layouts;
  unsigned definitions = 0, cfgFailures = 0;
  explicit Visitor(ASTContext &ctx) : ctx(ctx) {}
  bool VisitRecordDecl(RecordDecl *r) {
    if (!r->isCompleteDefinition() || !r->getIdentifier()) return true;
    auto name = r->getName();
    if (name != "_zval_struct" && name != "_zend_array" &&
        name != "_zend_reference" && name != "_zend_object_handlers" &&
        name != "Owner" && name != "Payload") return true;
    auto &layout = ctx.getASTRecordLayout(r);
    j::Array fields;
    unsigned i = 0;
    for (auto *f : r->fields()) {
      fields.push_back(j::Object{{"name", f->getNameAsString()},
        {"type", f->getType().getAsString()},
        {"offset_bits", static_cast<int64_t>(layout.getFieldOffset(i++))}});
    }
    layouts.push_back(j::Object{{"name", name},
      {"size_bytes", layout.getSize().getQuantity()}, {"fields", std::move(fields)}});
    return true;
  }
  bool VisitFunctionDecl(FunctionDecl *f) {
    if (!f->doesThisDeclarationHaveABody()) return true;
    ++definitions;
    if (!selected(f->getNameAsString())) return true;
    CFG::BuildOptions opts;
    opts.setAllAlwaysAdd();
    opts.PruneTriviallyFalseEdges = false;
    opts.AddEHEdges = true;
    opts.AddInitializers = true;
    opts.AddImplicitDtors = true;
    opts.AddTemporaryDtors = true;
    opts.AddLifetime = true;
    opts.AddScopes = true;
    opts.AddStaticInitBranches = true;
    opts.AddCXXNewAllocator = true;
    opts.AddRichCXXConstructors = true;
    auto cfg = CFG::buildCFG(f, f->getBody(), &ctx, opts);
    if (!cfg) { ++cfgFailures; return true; }
    j::Array blocks;
    unsigned elements = 0, edges = 0, direct = 0, indirect = 0, dtors = 0;
    for (auto *b : *cfg) {
      j::Array statements, successors;
      for (auto element : *b) {
        ++elements;
        j::Object record{{"element_kind", static_cast<int64_t>(element.getKind())}};
        if (auto stmt = element.getAs<CFGStmt>()) {
          auto *s = stmt->getStmt();
          record["kind"] = s->getStmtClassName();
          record["text"] = text(s, ctx);
          auto loc = ctx.getSourceManager().getExpansionLoc(s->getBeginLoc());
          record["line"] = ctx.getSourceManager().getPresumedLineNumber(loc);
          if (auto *call = dyn_cast<CallExpr>(s)) {
            if (auto *callee = call->getDirectCallee()) {
              record["callee"] = callee->getQualifiedNameAsString(); ++direct;
            } else { record["callee"] = "<indirect>"; ++indirect; }
            record["callee_type"] = call->getCallee()->getType().getAsString();
          }
          if (auto *member = dyn_cast<MemberExpr>(s)) {
            record["field"] = member->getMemberDecl()->getNameAsString();
            record["field_type"] = member->getType().getAsString();
          }
        } else if (auto dtor = element.getAs<CFGImplicitDtor>()) {
          record["kind"] = "ImplicitDtor";
          if (auto *decl = dtor->getDestructorDecl(ctx)) record["callee"] = decl->getQualifiedNameAsString();
          ++dtors;
        }
        statements.push_back(std::move(record));
      }
      for (auto next : b->succs()) {
        if (auto *reachable = next.getReachableBlock()) {
          successors.push_back(reachable->getBlockID()); ++edges;
        } else successors.push_back(nullptr);
      }
      blocks.push_back(j::Object{{"id", b->getBlockID()},
        {"condition", text(b->getTerminatorCondition(), ctx)},
        {"statements", std::move(statements)}, {"successors", std::move(successors)}});
    }
    j::Object result{{"name", f->getNameAsString()},
      {"type", f->getType().getAsString()}, {"entry", cfg->getEntry().getBlockID()},
      {"exit", cfg->getExit().getBlockID()}, {"block_count", cfg->size()},
      {"elements", elements}, {"edges", edges}, {"direct_calls", direct},
      {"indirect_calls", indirect}, {"implicit_dtors", dtors}, {"blocks", std::move(blocks)}};
    if (flowBudget) {
      unsigned visited = 0;
      dataflow::DiagnosisCallbacks<dataflow::NoopAnalysis, int> callbacks;
      auto diagnoseAfter = [&visited](const CFGElement &, ASTContext &,
          dataflow::TransferStateForDiagnostics<dataflow::NoopLattice>) {
        ++visited;
        return llvm::SmallVector<int>();
      };
      callbacks.After = diagnoseAfter;
      auto analysis = dataflow::diagnoseFunction<dataflow::NoopAnalysis, int>(
        *f, ctx, callbacks, 1000000, flowBudget);
      result["framework"] = "clang-flow-sensitive-noop-builtins";
      result["block_visit_limit"] = flowBudget;
      result["diagnosis_elements"] = visited;
      if (analysis) result["framework_status"] = "completed-no-catalog-contract";
      else {
        result["framework_status"] = "incomplete";
        result["framework_error"] = llvm::toString(analysis.takeError());
      }
    }
    functions.push_back(std::move(result));
    return true;
  }
};
class Consumer : public ASTConsumer {
public:
  void HandleTranslationUnit(ASTContext &ctx) override {
    Visitor visitor(ctx);
    visitor.TraverseDecl(ctx.getTranslationUnitDecl());
    llvm::outs() << j::Value(j::Object{{"representation", "clang-ast-cfg"},
      {"definitions", visitor.definitions}, {"cfg_failures", visitor.cfgFailures},
      {"functions", std::move(visitor.functions)}, {"layouts", std::move(visitor.layouts)}}) << '\n';
  }
};
class Action : public ASTFrontendAction {
  std::unique_ptr<ASTConsumer> CreateASTConsumer(CompilerInstance &, llvm::StringRef) override {
    return std::make_unique<Consumer>();
  }
};
static int probeIR(const char *path) {
  llvm::LLVMContext context;
  llvm::SMDiagnostic diagnostic;
  auto module = llvm::parseIRFile(path, diagnostic, context);
  if (!module) { diagnostic.print("native-probe", llvm::errs()); return 1; }
  j::Array functions;
  llvm::ModuleSlotTracker slots(module.get());
  unsigned definitions = 0;
  for (auto &f : *module) {
    if (f.isDeclaration()) continue;
    ++definitions;
    if (!selected(f.getName())) continue;
    slots.incorporateFunction(f);
    j::Array blocks;
    unsigned elements = 0, edges = 0, direct = 0, indirect = 0;
    for (auto &b : f) {
      j::Array instructions, successors;
      for (auto &i : b) {
        ++elements;
        std::string str;
        llvm::raw_string_ostream stream(str);
        i.print(stream, slots);
        j::Object record{{"kind", i.getOpcodeName()}, {"text", str}};
        if (auto loc = i.getDebugLoc()) record["line"] = loc.getLine();
        if (auto *call = llvm::dyn_cast<llvm::CallBase>(&i)) {
          if (auto *callee = call->getCalledFunction()) {
            record["callee"] = callee->getName(); ++direct;
          } else { record["callee"] = "<indirect>"; ++indirect; }
        }
        instructions.push_back(std::move(record));
      }
      auto *term = b.getTerminator();
      for (unsigned s = 0; s < term->getNumSuccessors(); ++s) {
        successors.push_back(term->getSuccessor(s)->getName()); ++edges;
      }
      blocks.push_back(j::Object{{"id", b.getName()},
        {"statements", std::move(instructions)}, {"successors", std::move(successors)}});
    }
    functions.push_back(j::Object{{"name", f.getName()}, {"block_count", f.size()},
      {"elements", elements}, {"edges", edges}, {"direct_calls", direct},
      {"indirect_calls", indirect}, {"blocks", std::move(blocks)}});
  }
  llvm::outs() << j::Value(j::Object{{"representation", "llvm"},
    {"definitions", definitions}, {"functions", std::move(functions)}}) << '\n';
  return 0;
}
int main(int argc, const char **argv) {
  if (argc < 4) { llvm::errs() << "usage: native-probe ast|ir entries-or-* file [-- compiler-args]\n"; return 2; }
  std::string names(argv[2]);
  if (names != "*") {
    size_t start = 0;
    do {
      size_t end = names.find(',', start);
      entries.insert(names.substr(start, end - start));
      if (end == std::string::npos) break;
      start = end + 1;
    } while (true);
  }
  if (std::string(argv[1]) == "ir") return probeIR(argv[3]);
  if (std::string(argv[1]) == "dataflow") flowBudget = 20000;
  if (std::string(argv[1]) == "dataflow-low-budget") flowBudget = 1;
  std::vector<std::string> args;
  for (int i = 4; i < argc; ++i) if (std::string(argv[i]) != "--") args.emplace_back(argv[i]);
  tooling::FixedCompilationDatabase database(".", args);
  tooling::ClangTool tool(database, {argv[3]});
  return tool.run(tooling::newFrontendActionFactory<Action>().get());
}
