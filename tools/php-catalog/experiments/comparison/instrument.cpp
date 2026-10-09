// Source/IR callsite identities and positive runtime target witnesses.
#include <llvm/ADT/StringExtras.h>
#include <llvm/IR/IRBuilder.h>
#include <llvm/IR/Instructions.h>
#include <llvm/IR/DebugInfoMetadata.h>
#include <llvm/IRReader/IRReader.h>
#include <llvm/Bitcode/BitcodeWriter.h>
#include <llvm/Support/JSON.h>
#include <llvm/Support/SourceMgr.h>
#include <llvm/Support/FileSystem.h>
#include <llvm/Support/raw_ostream.h>

namespace j = llvm::json;
int main(int argc, char **argv) {
  if (argc != 4) return 2;
  llvm::LLVMContext context;
  llvm::SMDiagnostic diagnostic;
  auto module = llvm::parseIRFile(argv[1], diagnostic, context);
  if (!module) { diagnostic.print(argv[0], llvm::errs()); return 1; }
  auto hook = module->getOrInsertFunction("__php_catalog_target", llvm::FunctionType::get(
    llvm::Type::getVoidTy(context), {llvm::Type::getInt64Ty(context), llvm::PointerType::getUnqual(context)}, false));
  j::Array sites;
  uint64_t next = 1;
  unsigned definitions = 0, instructions = 0;
  j::Object opcodes;
  for (auto &function : *module) {
    if (function.isDeclaration()) continue;
    ++definitions;
    for (auto &block : function) {
      for (auto it = block.begin(); it != block.end();) {
        auto &instruction = *it++;
        ++instructions;
        auto name = instruction.getOpcodeName();
        auto old = opcodes.getInteger(name);
        opcodes[name] = old.value_or(0) + 1;
        auto *call = llvm::dyn_cast<llvm::CallBase>(&instruction);
        if (!call || call->isInlineAsm() ||
            llvm::isa<llvm::Function>(call->getCalledOperand()->stripPointerCasts())) continue;
        llvm::IRBuilder<> builder(call);
        auto *target = builder.CreatePointerCast(call->getCalledOperand(), llvm::PointerType::getUnqual(context));
        builder.CreateCall(hook, {builder.getInt64(next), target});
        j::Object site{{"id", static_cast<int64_t>(next++)}, {"function", function.getName()}};
        if (auto loc = instruction.getDebugLoc()) {
          site["line"] = loc.getLine(); site["column"] = loc.getCol();
          site["file"] = loc->getFilename(); site["directory"] = loc->getDirectory();
        }
        sites.push_back(std::move(site));
      }
    }
  }
  std::error_code error;
  llvm::raw_fd_ostream bitcode(argv[2], error, llvm::sys::fs::OF_None);
  if (error) return 1;
  llvm::WriteBitcodeToFile(*module, bitcode);
  llvm::raw_fd_ostream metadata(argv[3], error);
  if (error) return 1;
  metadata << j::Value(j::Object{{"definitions", definitions}, {"instructions", instructions},
    {"opcodes", std::move(opcodes)}, {"sites", std::move(sites)},
    {"evidence_kind", "positive-observation-only"}}) << '\n';
  return 0;
}
