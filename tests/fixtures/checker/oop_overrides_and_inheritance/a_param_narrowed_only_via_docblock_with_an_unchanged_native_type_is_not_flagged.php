<?php
            class Node4 {}
            class SpecificNode4 extends Node4 {}
            interface Rule4 { public function refactor(Node4 $node): ?Node4; }
            class MyRector4 implements Rule4 {
                /** @param SpecificNode4 $node */
                public function refactor(Node4 $node): ?Node4 { return $node; }
            }
        