import React from 'react';
import { createPortal } from 'react-dom';
import { 
    DndContext, 
    closestCenter, 
    KeyboardSensor, 
    PointerSensor, 
    useSensor, 
    useSensors, 
    DragOverlay, 
    defaultDropAnimationSideEffects, 
    DragStartEvent, 
    DragOverEvent 
} from '@dnd-kit/core';
import { 
    arrayMove, 
    SortableContext, 
    sortableKeyboardCoordinates, 
    useSortable, 
    rectSortingStrategy 
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { TemplateBlock } from '@/types';
import { Plus, GripVertical, Trash2 } from 'lucide-react';
import { twMerge } from 'tailwind-merge';
import { v4 as uuidv4 } from 'uuid';

// --- Available Blocks Definition ---

const AVAILABLE_VARS: Omit<TemplateBlock, 'id'>[] = [
    { type: 'variable', value: 'title', label: 'Title' },
    { type: 'variable', value: 'id', label: 'Video ID' },
    { type: 'variable', value: 'uploader', label: 'Uploader' },
    { type: 'variable', value: 'upload_date', label: 'Date' },
    { type: 'variable', value: 'resolution', label: 'Resolution' },
    { type: 'variable', value: 'duration', label: 'Duration' },
    { type: 'variable', value: 'ext', label: 'Extension' },
];

const AVAILABLE_SEPARATORS: Omit<TemplateBlock, 'id'>[] = [
    { type: 'separator', value: '.', label: '.' },
    { type: 'separator', value: ' - ', label: ' - ' },
    { type: 'separator', value: '_', label: '_' },
    { type: 'separator', value: ' ', label: '(Space)' },
];


// --- Sub Components ---

interface BlockProps extends React.HTMLAttributes<HTMLDivElement> {
    block: TemplateBlock;
    isOverlay?: boolean;
    onRemove?: () => void;
}

const Block = React.forwardRef<HTMLDivElement, BlockProps>(({ block, isOverlay, className, onRemove, ...props }, ref) => {
    const isVar = block.type === 'variable';
    
    return (
        <div
            ref={ref}
            className={twMerge(
                "relative flex items-center gap-2 px-3 py-2 rounded-md border text-[10px] font-bold uppercase tracking-wider select-none transition-all",
                "whitespace-nowrap flex-shrink-0 min-w-[60px] h-8", // Defect #3: Fixed height and min-width
                isVar ? "bg-theme-cyan/10 text-theme-cyan border-theme-cyan/30" : "bg-zinc-800 text-zinc-400 border-zinc-700",
                isOverlay ? "shadow-2xl scale-110 cursor-grabbing z-50 border-theme-cyan/50" : "cursor-grab active:cursor-grabbing",
                className
            )}
            {...props}
        >
            <GripVertical className="h-3 w-3 opacity-50" />
            <span>{block.label}</span>
            {onRemove && (
                <button onClick={(e) => { e.stopPropagation(); onRemove(); }} className="ml-auto hover:text-red-500 transition-colors pl-2">
                    <Trash2 className="h-3 w-3" />
                </button>
            )}
        </div>
    );
});
Block.displayName = "Block";

const SortableBlock = ({ block, onRemove }: { block: TemplateBlock, onRemove: (id: string) => void }) => {
    const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({ id: block.id });
    
    const style = {
        transform: CSS.Translate.toString(transform),
        transition,
        opacity: isDragging ? 0.3 : 1,
        // Defect #3: Ensure no internal layout shift during transition
        zIndex: isDragging ? 50 : 1,
    };

    return (
        <Block 
            ref={setNodeRef} 
            style={style} 
            block={block} 
            onRemove={() => onRemove(block.id)}
            {...attributes} 
            {...listeners} 
        />
    );
};


// --- Main Component ---

interface TemplateEditorProps {
    blocks: TemplateBlock[];
    onChange: (blocks: TemplateBlock[]) => void;
}

export function TemplateEditor({ blocks, onChange }: TemplateEditorProps) {
    const [activeId, setActiveId] = React.useState<string | null>(null);

    const sensors = useSensors(
        useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
        useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates })
    );

    const handleDragStart = (event: DragStartEvent) => {
        setActiveId(event.active.id as string);
    };

    const handleDragOver = (event: DragOverEvent) => {
        const { active, over } = event;
        if (over && active.id !== over.id) {
            const oldIndex = blocks.findIndex((b) => b.id === active.id);
            const newIndex = blocks.findIndex((b) => b.id === over.id);

            if (oldIndex !== newIndex) {
                onChange(arrayMove(blocks, oldIndex, newIndex));
            }
        }
    };

    const handleDragEnd = () => {
        setActiveId(null);
    };

    const addBlock = (base: Omit<TemplateBlock, 'id'>) => {
        const newBlock = { ...base, id: uuidv4() };
        onChange([...blocks, newBlock]);
    };

    const removeBlock = (id: string) => {
        onChange(blocks.filter(b => b.id !== id));
    };

    const activeBlock = blocks.find(b => b.id === activeId);

    // Preview string generation
    const previewString = blocks.map(b => b.type === 'variable' ? `[${b.label}]` : b.label).join('');

    return (
        <div className="space-y-6">
            {/* Preview Section */}
            <div className="bg-zinc-950 p-4 rounded-lg border border-zinc-800">
                <div className="text-[10px] text-zinc-600 mb-2 uppercase font-black tracking-widest">Output Preview</div>
                <div className="font-mono text-sm text-zinc-300 break-all bg-black/30 p-2 rounded border border-white/5">
                    {previewString || <span className="text-zinc-600 italic">Empty template...</span>}
                </div>
            </div>

            {/* Editor Area */}
            <div className="space-y-2">
                 <div className="text-[10px] text-zinc-600 uppercase font-black tracking-widest">Active Template (Drag to reorder)</div>
                 {/* Defect #3: Stable flex container with specific gap to prevent jitter */}
                 <div className="min-h-[100px] p-4 bg-zinc-900/50 border border-dashed border-zinc-700 rounded-lg flex flex-wrap gap-2.5 items-center content-start">
                    <DndContext 
                        sensors={sensors} 
                        collisionDetection={closestCenter} 
                        onDragStart={handleDragStart} 
                        onDragOver={handleDragOver}
                        onDragEnd={handleDragEnd}
                    >
                        <SortableContext items={blocks} strategy={rectSortingStrategy}>
                            {blocks.map((block) => (
                                <SortableBlock key={block.id} block={block} onRemove={removeBlock} />
                            ))}
                        </SortableContext>
                        
                        {blocks.length === 0 && (
                            <div className="w-full text-center text-zinc-600 text-sm py-4">
                                Drag and drop is ready. Add blocks from below.
                            </div>
                        )}

                        {createPortal(
                            <DragOverlay dropAnimation={{
                                sideEffects: defaultDropAnimationSideEffects({ styles: { active: { opacity: '0.5' } } }),
                            }}>
                                {activeId && activeBlock ? <Block block={activeBlock} isOverlay /> : null}
                            </DragOverlay>,
                            document.body
                        )}
                    </DndContext>
                 </div>
            </div>

            {/* Toolbox */}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                <div className="space-y-3">
                    <div className="text-[10px] text-zinc-600 uppercase font-black tracking-widest">Available Variables</div>
                    <div className="flex flex-wrap gap-2">
                        {AVAILABLE_VARS.map((v) => (
                            <button 
                                key={v.value}
                                onClick={() => addBlock(v)}
                                className="group flex items-center gap-2 px-3 py-2 rounded-md border border-zinc-800 bg-zinc-900 hover:border-theme-cyan/50 hover:bg-zinc-800 transition-all"
                            >
                                <Plus className="h-3 w-3 text-zinc-500 group-hover:text-theme-cyan" />
                                <span className="text-xs font-medium text-zinc-300 group-hover:text-zinc-100">{v.label}</span>
                            </button>
                        ))}
                    </div>
                </div>

                <div className="space-y-3">
                    <div className="text-[10px] text-zinc-600 uppercase font-black tracking-widest">Separators</div>
                    <div className="flex flex-wrap gap-2">
                        {AVAILABLE_SEPARATORS.map((s) => (
                            <button 
                                key={s.label}
                                onClick={() => addBlock(s)}
                                className="group flex items-center gap-2 px-3 py-2 rounded-md border border-zinc-800 bg-zinc-900 hover:border-zinc-600 hover:bg-zinc-800 transition-all"
                            >
                                <Plus className="h-3 w-3 text-zinc-500 group-hover:text-zinc-300" />
                                <span className="text-xs font-mono text-zinc-400 group-hover:text-zinc-200">{s.label}</span>
                            </button>
                        ))}
                    </div>
                </div>
            </div>
        </div>
    );
}