<?php
            function hideBlock(string $block_id): void {}
            /** @param array<int> $blocks */
            function apply(array $blocks): void {
                foreach ($blocks as $id => $block) {
                    hideBlock($id);
                }
            }
        