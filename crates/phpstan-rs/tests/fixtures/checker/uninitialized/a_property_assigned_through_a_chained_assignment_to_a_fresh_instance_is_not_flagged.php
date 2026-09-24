<?php
            class Tag { public $node; }
            class Node {
                public bool $postTest;
                public static function create(Tag $tag) {
                    $node = $tag->node = new static;
                    $node->postTest = true;
                    return $node;
                }
            }
        