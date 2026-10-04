<?php
namespace NsA;
class A {}
function make(): object { return new class extends \NsB\B {}; }
