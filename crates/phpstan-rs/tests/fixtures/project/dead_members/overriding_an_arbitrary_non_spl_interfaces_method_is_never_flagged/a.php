<?php interface NodeVisitor { public function enterNode(): void; }
class MyVisitor implements NodeVisitor { public function enterNode(): void {} }