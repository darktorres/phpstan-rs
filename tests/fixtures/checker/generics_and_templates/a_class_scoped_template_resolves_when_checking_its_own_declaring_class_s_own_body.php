<?php
            interface Node17 {}
            /** @template TNodeType of Node17 */
            interface Rule17 {
                /** @param TNodeType $node */
                public function processNode(Node17 $node): void;
            }
            final class StaticCall17 implements Node17 {}
            /** @implements Rule17<StaticCall17> */
            final class InvalidRegexPatternRule17 implements Rule17 {
                public function processNode(Node17 $node): void {
                    $this->extractPatterns($node);
                }
                private function extractPatterns(StaticCall17 $node): void {}
            }
        