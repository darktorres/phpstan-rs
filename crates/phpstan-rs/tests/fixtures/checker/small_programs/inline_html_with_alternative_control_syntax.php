<?php
class Point4
{
    public int $x = 0;
    public int $y = 0;
}

$p1 = new Point4();
$p2 = clone $p1;
$p2->x = 5;
?>
<ul>
<?php if ($p2->x > 0): ?>
    <li>positive</li>
<?php elseif ($p2->x === 0): ?>
    <li>zero</li>
<?php else: ?>
    <li>negative</li>
<?php endif; ?>
</ul>
<?php
$i = 0;
while ($i < 3):
    echo $i;
    $i++;
endwhile;

for ($j = 0; $j < 3; $j++):
    echo $j;
endfor;

$items = [1, 2, 3];
foreach ($items as $item):
    echo $item;
endforeach;

switch ($p2->x):
    case 5:
        echo "five";
        break;
    default:
        echo "other";
endswitch;
