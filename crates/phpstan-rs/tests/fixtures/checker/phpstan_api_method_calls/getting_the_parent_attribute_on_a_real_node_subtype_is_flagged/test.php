<?php
        namespace PhpParser {
            class Node {}
        }
        namespace PhpParser\Node\Stmt {
            class ClassMethod extends \PhpParser\Node {}
        }
        namespace App {
            class MyRule implements \PHPStan\Rules\Rule {
                public function check(\PhpParser\Node\Stmt\ClassMethod $node) {
                    $node->getAttribute('previous');
                }
            }
        }
        