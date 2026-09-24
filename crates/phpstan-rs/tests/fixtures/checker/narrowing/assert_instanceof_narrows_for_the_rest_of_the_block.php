<?php
            class Node {}
            class FuncCall extends Node { public array $args = []; }
            function mutate(Node $node) {
                assert($node instanceof FuncCall);
                return $node->args;
            }
        