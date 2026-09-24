<?php
            class Definition {
                public function getClass(): ?string { return null; }
                public function isChild(): bool { return false; }
            }
            function f(Definition $definition): void {
                while ((null === $class = $definition->getClass()) && $definition->isChild()) {
                    echo "loop";
                }
                echo $class;
            }
        