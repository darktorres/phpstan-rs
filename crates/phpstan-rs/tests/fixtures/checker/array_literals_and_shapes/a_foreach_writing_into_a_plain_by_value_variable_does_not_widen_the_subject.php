<?php
            /**
             * @param array<int, array{id: int}> $infos
             */
            function test(array $infos): void {
                foreach ($infos as $info) {
                    $info['src_image'] = 'x';
                }

                $rep = $infos[1] ?? null;
                $x = $rep === null ? null : $rep['src_image'];
            }
        