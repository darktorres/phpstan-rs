<?php
            class DisplayBlock {}
            function hideBlock(string $block_id): void {}
            /** @param DisplayBlock[] $blocks */
            function apply(array $blocks): void {
                foreach ($blocks as $id => $block) {
                    hideBlock($id);
                }
            }
        