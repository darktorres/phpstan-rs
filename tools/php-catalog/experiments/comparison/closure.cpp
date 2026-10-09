// Audit all direct-call dependencies; indirect/absent code stays explicitly open.
#include <llvm/IR/Instructions.h>
#include <llvm/IR/Module.h>
#include <llvm/IRReader/IRReader.h>
#include <llvm/Support/JSON.h>
#include <llvm/Support/SourceMgr.h>
#include <llvm/Support/raw_ostream.h>
#include <set>
#include <vector>

namespace j = llvm::json;
int main(int argc, char **argv) {
  if (argc != 3) return 2;
  llvm::LLVMContext context;
  llvm::SMDiagnostic diagnostic;
  auto module = llvm::parseIRFile(argv[1], diagnostic, context);
  if (!module) { diagnostic.print(argv[0], llvm::errs()); return 1; }
  std::string names(argv[2]);
  j::Array roots;
  size_t at = 0;
  do {
    size_t end = names.find(',', at);
    std::string name = names.substr(at, end-at);
    auto *root = module->getFunction(name);
    std::set<std::string> missing, intrinsics;
    std::set<llvm::Function *> visited;
    std::vector<llvm::Function *> work;
    unsigned operations = 0, indirect = 0, assembly = 0;
    if (root && !root->isDeclaration()) work.push_back(root);
    while (!work.empty()) {
      auto *f = work.back(); work.pop_back();
      if (!visited.insert(f).second) continue;
      for (auto &block : *f) for (auto &instruction : block) {
        ++operations;
        auto *call = llvm::dyn_cast<llvm::CallBase>(&instruction);
        if (!call) continue;
        if (call->isInlineAsm()) { ++assembly; continue; }
        auto *callee = llvm::dyn_cast<llvm::Function>(call->getCalledOperand()->stripPointerCasts());
        if (!callee) { ++indirect; continue; }
        if (callee->isIntrinsic()) intrinsics.insert(callee->getName().str());
        else if (callee->isDeclaration()) missing.insert(callee->getName().str());
        else work.push_back(callee);
      }
    }
    j::Array missingArray, intrinsicArray;
    for (auto &s : missing) missingArray.push_back(s);
    for (auto &s : intrinsics) intrinsicArray.push_back(s);
    roots.push_back(j::Object{{"root", name}, {"found", root != nullptr},
      {"direct_reachable_definitions", static_cast<int64_t>(visited.size())},
      {"instructions", operations}, {"indirect_call_sites", indirect}, {"inline_assembly_sites", assembly},
      {"missing_native_bodies", std::move(missingArray)}, {"intrinsic_contracts_required", std::move(intrinsicArray)},
      {"catalog_semantic_closure", false}, {"audit", "direct-call dependency inventory; not a semantic solver"}});
    if (end == std::string::npos) break;
    at = end + 1;
  } while (true);
  llvm::outs() << j::Value(std::move(roots)) << '\n';
}
