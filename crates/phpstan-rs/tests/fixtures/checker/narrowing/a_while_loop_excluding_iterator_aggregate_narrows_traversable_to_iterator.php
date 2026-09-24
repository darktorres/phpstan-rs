<?php
            function handle(Traversable $x): void {
                while ($x instanceof IteratorAggregate) {
                    $x = $x->getIterator();
                }
                $x->rewind();
                if ($x->valid()) {
                    $x->current();
                }
            }
        